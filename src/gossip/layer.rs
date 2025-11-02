use super::member_list::{MemberList, SharedMemberList};
use super::messages::{GossipMessage, Member, MemberId, MemberState, MemberUpdate};
use anyhow::Result;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

pub struct GossipLayer {
    member_list: SharedMemberList,
    socket: Arc<UdpSocket>,
    pending_pings: Arc<Mutex<HashSet<MemberId>>>,
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
            pending_pings: Arc::new(Mutex::new(HashSet::new())),
        };

        Ok((gossip_layer, member_list))
    }

    pub fn socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    pub fn pending_pings(&self) -> Arc<Mutex<HashSet<MemberId>>> {
        self.pending_pings.clone()
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
                    let mut pending = self.pending_pings.lock().await;
                    pending.remove(&from);
                }

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

    pub async fn start_gossip_loop(
        member_list: SharedMemberList,
        socket: Arc<UdpSocket>,
        pending_pings: Arc<Mutex<HashSet<MemberId>>>,
        gossip_interval: Duration,
        ping_timeout: Duration,
    ) {
        let mut interval = tokio::time::interval(gossip_interval);
        let mut tick_count = 0;

        loop {
            interval.tick().await;
            tick_count += 1;

            {
                let mut members = member_list.write().await;
                members.check_suspect_timeouts();
            }

            if tick_count % 30 == 0 {
                let mut members = member_list.write().await;
                members.prune_dead_members(Duration::from_secs(60));
                info!("Pruned dead members from list");
            }

            {
                let mut pending = pending_pings.lock().await;
                if !pending.is_empty() {
                    let mut members = member_list.write().await;
                    for member_id in pending.drain() {
                        warn!("No ACK from {} - marking as suspect", member_id.0);
                        members.mark_suspect(&member_id);
                    }
                }
            }

            let target = {
                let members = member_list.read().await;
                members.get_random_alive_member()
            };

            if let Some(target_member) = target {
                {
                    let mut pending = pending_pings.lock().await;
                    pending.insert(target_member.id.clone());
                }
                let local_info = {
                    let members = member_list.read().await;
                    let local = members.local_member();
                    (
                        local.id.clone(),
                        local.addr,
                        local.incarnation,
                        members.get_member_updates(5),
                    )
                };

                let ping = GossipMessage::Ping {
                    from: local_info.0.clone(),
                    from_addr: local_info.1,
                    incarnation: local_info.2,
                    member_updates: local_info.3,
                    backend_updates: vec![],
                };

                debug!(
                    "Pinging member {} at {}",
                    target_member.id.0, target_member.addr
                );

                if let Ok(bytes) = ping.to_bytes() {
                    if let Err(e) = socket.send_to(&bytes, target_member.addr).await {
                        warn!("Failed to send ping to {}: {}", target_member.addr, e);
                        let mut pending = pending_pings.lock().await;
                        pending.remove(&target_member.id);
                    }
                }
            }
        }
    }

    pub async fn join_cluster(&self, seed_nodes: Vec<SocketAddr>) {
        if seed_nodes.is_empty() {
            info!("No seed nodes configured - starting as initial cluster member");
            return;
        }

        info!("Joining cluster via {} seed nodes", seed_nodes.len());

        for seed_addr in seed_nodes {
            let (ping_msg, local_addr) = {
                let members = self.member_list.read().await;
                let local = members.local_member();
                let ping = GossipMessage::Ping {
                    from: local.id.clone(),
                    from_addr: local.addr,
                    incarnation: local.incarnation,
                    member_updates: vec![],
                    backend_updates: vec![],
                };

                (ping, local.addr)
            };

            if seed_addr == local_addr {
                continue;
            }

            info!("Contacting seed node at {}", seed_addr);

            if let Ok(bytes) = ping_msg.to_bytes() {
                match self.socket.send_to(&bytes, seed_addr).await {
                    Ok(_) => {
                        info!("Sent join request to {}", seed_addr);
                    }
                    Err(e) => {
                        warn!("Failed to contact seed node {}: {}", seed_addr, e);
                    }
                }
            }
        }
        info!("Cluster join initiated");
    }
}
