use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemberId(pub String);

impl MemberId {
    pub fn new(name: String) -> Self {
        Self(name)
    }

    pub fn generate(addr: SocketAddr) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Self(format!("flux-{}-{}", addr.port(), timestamp))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberState {
    Alive,
    Suspect,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: MemberId,
    pub addr: SocketAddr,
    pub state: MemberState,
    pub incarnation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendHealthInfo {
    pub addr: SocketAddr,
    pub is_healthy: bool,
    pub timestamp: u64, // When this observation was made
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberUpdate {
    pub member_id: MemberId,
    pub addr: SocketAddr,
    pub state: MemberState,
    pub incarnation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendUpdate {
    pub backend_addr: SocketAddr,
    pub is_healthy: bool,
    pub from_member: MemberId,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    Ping {
        from: MemberId,
        from_addr: SocketAddr,
        incarnation: u64,
        member_updates: Vec<MemberUpdate>,
        backend_updates: Vec<BackendUpdate>,
    },

    Ack {
        from: MemberId,
        from_addr: SocketAddr,
        incarnation: u64,
        member_updates: Vec<MemberUpdate>,
        backend_updates: Vec<BackendUpdate>,
    },

    IndirectPing {
        from: MemberId,
        from_addr: SocketAddr,
        target_id: MemberId,
        target_addr: SocketAddr,
    },

    IndirectAck {
        from: MemberId,
        target_id: MemberId,
        target_responded: bool,
    },
}

impl GossipMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let bytes = bincode::serialize(self)?;
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let message = bincode::deserialize(bytes)?;
        Ok(message)
    }
}

pub struct GossipLayer {
    member_list: SharedMemberList,
    socket: Arc<UdpSocket>,
}

impl GossipLayer {
    pub async fn new(
        local_id: MemberId,
        bind_addr: SocketAddr,
        suspect_timeout: Duration,
    ) -> Result<(Self, SharedMemberList)> {
        let socket = UdpSocket::bind(bind_addr).await?;
        debug!("Gossip layer bound to {}", bind_addr);

        let local_member = Member {
            id: local_id,
            addr: bind_addr,
            state: MemberState::Alive,
            incarnation: 0,
        };

        let member_list = Arc::new(RwLock::new(MemberList::new(local_member, suspect_timeout)));

        let gossip_layer = Self {
            member_list: member_list.clone(),
            socket: Arc::new(socket),
        };

        Ok((gossip_layer, member_list))
    }

    pub async fn send_message(&self, message: GossipMessage, target: SocketAddr) -> Result<()> {
        let bytes = message.to_bytes()?;
        self.socket.send_to(&bytes, target).await?;
        debug!("Sent {:?} to {}", message, target);
        Ok(())
    }

    pub async fn run(&mut self) {
        let mut buf = vec![0u8; 65535]; // Max UDP packet size

        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    let data = &buf[..len];

                    match GossipMessage::from_bytes(data) {
                        Ok(message) => {
                            debug!("Received {:?} from {}", message, src_addr);

                            if let Err(e) = self.handle_message(message, src_addr).await {
                                error!("Error handling message from {}: {}", src_addr, e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to deserialize message from {}: {}", src_addr, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving UDP message: {}", e);
                }
            }
        }
    }

    async fn handle_message(&mut self, message: GossipMessage, src_addr: SocketAddr) -> Result<()> {
        match message {
            GossipMessage::Ping {
                from,
                from_addr,
                incarnation,
                member_updates,
                backend_updates,
            } => {
                debug!("Handling Ping from {}", from.0);

                self.process_member_updates(member_updates).await;
                
                {
                    let mut members = self.member_list.write().await;
                    members.upsert_member(Member {
                        id: from.clone(),
                        addr: from_addr,
                        state: MemberState::Alive,
                        incarnation,
                    });
                }

                let (ack_from, ack_addr, ack_incarnation, updates) = {
                    let members = self.member_list.read().await;
                    let local = members.local_member();
                    (
                        local.id.clone(),
                        local.addr,
                        local.incarnation,
                        members.get_member_updates(5),
                    )
                };

                let ack = GossipMessage::Ack {
                    from: ack_from,
                    from_addr: ack_addr,
                    incarnation: ack_incarnation,
                    member_updates: updates,
                    backend_updates: vec![],
                };

                self.send_message(ack, from_addr).await?;
            }

            GossipMessage::Ack {
                from,
                from_addr,
                incarnation,
                member_updates,
                backend_updates,
            } => {
                debug!("Handling Ack from {}", from.0);

                {
                    let mut members = self.member_list.write().await;
                    members.upsert_member(Member {
                        id: from.clone(),
                        addr: from_addr,
                        state: MemberState::Alive,
                        incarnation,
                    });
                }

                self.process_member_updates(member_updates).await;

                // TODO: process backend_updates
            }

            GossipMessage::IndirectPing { .. } => {
                // TODO: implement indirect ping
            }

            GossipMessage::IndirectAck { .. } => {
                // TODO: implement indirect ack
            }
        }

        Ok(())
    }

    async fn process_member_updates(&self, updates: Vec<MemberUpdate>) {
        let mut members = self.member_list.write().await;
        
        for update in updates {
            members.upsert_member(Member {
                id: update.member_id,
                addr: update.addr,
                state: update.state,
                incarnation: update.incarnation,
            });
        }
    }

}

#[derive(Debug, Clone)]
struct MemberInfo {
    member: Member,
    last_seen: Instant,
    suspect_at: Option<Instant>,
}

impl MemberInfo {
    fn new(member: Member) -> Self {
        Self {
            member,
            last_seen: Instant::now(),
            suspect_at: None,
        }
    }
}

pub struct MemberList {
    local_member: Member,
    members: HashMap<MemberId, MemberInfo>,
    suspect_timeout: Duration,
}

impl MemberList {
    pub fn new(local_member: Member, suspect_timeout: Duration) -> Self {
        let mut members = HashMap::new();

        // add ourselves to the member list
        members.insert(
            local_member.id.clone(),
            MemberInfo::new(local_member.clone()),
        );

        Self {
            local_member,
            members,
            suspect_timeout,
        }
    }

    pub fn upsert_member(&mut self, member: Member) {
        let member_id = member.id.clone();

        if let Some(existing) = self.members.get_mut(&member_id) {
            if member.incarnation > existing.member.incarnation {
                debug!(
                    "Updating member {} from incarnation {} to {}",
                    member_id.0, existing.member.incarnation, member.incarnation
                );
                existing.member = member;
                existing.last_seen = Instant::now();
                existing.suspect_at = None;
            } else if member.incarnation == existing.member.incarnation {
                existing.last_seen = Instant::now();

                if member.state != existing.member.state {
                    existing.member.state = member.state;
                }
            }
        } else {
            // new member
            info!("Discovered new member: {} at {}", member_id.0, member.addr);
            self.members.insert(member_id, MemberInfo::new(member));
        }
    }

    pub fn mark_alive(&mut self, member_id: &MemberId) {
        if let Some(info) = self.members.get_mut(member_id) {
            if info.member.state != MemberState::Alive {
                info!("Member {} is now ALIVE", member_id.0);
                info.member.state = MemberState::Alive;
            }
            info.last_seen = Instant::now();
            info.suspect_at = None;
        }
    }

    pub fn mark_suspect(&mut self, member_id: &MemberId) {
        if let Some(info) = self.members.get_mut(member_id) {
            if info.member.state == MemberState::Alive {
                warn!("Member {} is now SUSPECT", member_id.0);
                info.member.state = MemberState::Suspect;
                info.suspect_at = Some(Instant::now());
            }
        }
    }

    pub fn mark_dead(&mut self, member_id: &MemberId) {
        if let Some(info) = self.members.get_mut(member_id) {
            if info.member.state != MemberState::Dead {
                warn!("Member {} is now DEAD", member_id.0);
                info.member.state = MemberState::Dead;
            }
        }
    }

    pub fn get_alive_members(&self) -> Vec<Member> {
        self.members
            .values()
            .filter(|info| {
                info.member.state == MemberState::Alive && info.member.id != self.local_member.id
            })
            .map(|info| info.member.clone())
            .collect()
    }

    pub fn get_all_members(&self) -> Vec<Member> {
        self.members
            .values()
            .filter(|info| info.member.id != self.local_member.id)
            .map(|info| info.member.clone())
            .collect()
    }

    pub fn get_random_alive_member(&self) -> Option<Member> {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();

        let alive: Vec<_> = self.get_alive_members();
        alive.choose(&mut rng).cloned()
    }

    pub fn check_suspect_timeouts(&mut self) {
        let now = Instant::now();
        let suspect_timeout = self.suspect_timeout;

        let to_mark_dead: Vec<MemberId> = self
            .members
            .iter()
            .filter_map(|(id, info)| {
                if info.member.state == MemberState::Suspect {
                    if let Some(suspect_at) = info.suspect_at {
                        if now.duration_since(suspect_at) > suspect_timeout {
                            return Some(id.clone());
                        }
                    }
                }
                None
            })
            .collect();

        for member_id in to_mark_dead {
            self.mark_dead(&member_id);
        }
    }

    pub fn prune_dead_members(&mut self, dead_timeout: Duration) {
        let now = Instant::now();

        self.members.retain(|id, info| {
            if info.member.state == MemberState::Dead {
                let time_dead = now.duration_since(info.last_seen);
                if time_dead > dead_timeout {
                    info!("Pruning dead member: {}", id.0);
                    return false;
                }
            }
            true
        });
    }

    pub fn get_member_updates(&self, max_count: usize) -> Vec<MemberUpdate> {
        // for now, just return recent state changes
        // todo: track which updates each peer has seen
        self.members
            .values()
            .take(max_count)
            .map(|info| MemberUpdate {
                member_id: info.member.id.clone(),
                addr: info.member.addr,
                state: info.member.state,
                incarnation: info.member.incarnation,
            })
            .collect()
    }

    pub fn local_member(&self) -> &Member {
        &self.local_member
    }

    pub fn increment_incarnation(&mut self) {
        self.local_member.incarnation += 1;
        if let Some(info) = self.members.get_mut(&self.local_member.id) {
            info.member.incarnation = self.local_member.incarnation;
        }
        info!(
            "Incremented local incarnation to {}",
            self.local_member.incarnation
        );
    }
}

pub type SharedMemberList = Arc<RwLock<MemberList>>;
