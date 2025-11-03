use super::member_list::{MemberList, SharedMemberList};
use super::messages::{BackendUpdate, GossipMessage, Member, MemberId, MemberState, MemberUpdate};
use super::states::IndirectPingState;
use crate::backend::SharedBackendPool;
use anyhow::Result;
use rand::thread_rng;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

pub struct GossipLayer {
    member_list: SharedMemberList,
    socket: Arc<UdpSocket>,
    pending_pings: Arc<Mutex<HashSet<MemberId>>>,
    pending_indirect_pings: Arc<Mutex<HashMap<MemberId, IndirectPingState>>>,
    backend_pool: SharedBackendPool,
}

impl GossipLayer {
    pub async fn new(
        local_id: MemberId,
        bind_addr: SocketAddr,
        suspect_timeout: Duration,
        backend_pool: SharedBackendPool,
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
            backend_pool,
            pending_indirect_pings: Arc::new(Mutex::new(HashMap::new())),
        };

        Ok((gossip_layer, member_list))
    }

    pub fn socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    pub fn pending_pings(&self) -> Arc<Mutex<HashSet<MemberId>>> {
        self.pending_pings.clone()
    }

    pub fn pending_indirect_pings(&self) -> Arc<Mutex<HashMap<MemberId, IndirectPingState>>> {
        self.pending_indirect_pings.clone()
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
                self.process_backend_updates(backend_updates).await;

                {
                    let mut members = self.member_list.write().await;
                    members.upsert_member(Member {
                        id: from.clone(),
                        addr: from_addr,
                        state: MemberState::Alive,
                        incarnation,
                    });
                }

                let (ack_from, ack_addr, ack_incarnation, updates, backend_updates) = {
                    let members = self.member_list.read().await;
                    let backends = self.backend_pool.lock().await;

                    let local = members.local_member();
                    let mut backend_updates = backends.get_backend_health_updates();
                    for update in &mut backend_updates {
                        update.from_member = local.id.clone();
                    }

                    (
                        local.id.clone(),
                        local.addr,
                        local.incarnation,
                        members.get_member_updates(5),
                        backend_updates,
                    )
                };

                let ack = GossipMessage::Ack {
                    from: ack_from,
                    from_addr: ack_addr,
                    incarnation: ack_incarnation,
                    member_updates: updates,
                    backend_updates,
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
                self.process_backend_updates(backend_updates).await;
            }

            GossipMessage::IndirectPing {
                from,
                from_addr,
                target_id,
                target_addr,
            } => {
                debug!(
                    "Handling IndirectPing request from {} to ping {}",
                    from.0, target_id.0
                );

                let socket = self.socket.clone();
                let member_list = self.member_list.clone();
                let backend_pool = self.backend_pool.clone();
                let pending_pings = self.pending_pings.clone();

                tokio::spawn(async move {
                    let local_info = {
                        let members = member_list.read().await;
                        let backends = backend_pool.lock().await;
                        let local = members.local_member();

                        let mut backend_updates = backends.get_backend_health_updates();
                        for update in &mut backend_updates {
                            update.from_member = local.id.clone();
                        }

                        (
                            local.id.clone(),
                            local.addr,
                            local.incarnation,
                            members.get_member_updates(3),
                            backend_updates,
                        )
                    };

                    {
                        let mut pending = pending_pings.lock().await;
                        pending.insert(target_id.clone());
                    }

                    let ping = GossipMessage::Ping {
                        from: local_info.0.clone(),
                        from_addr: local_info.1,
                        incarnation: local_info.2,
                        member_updates: local_info.3,
                        backend_updates: local_info.4,
                    };

                    if let Ok(bytes) = ping.to_bytes() {
                        let _ = socket.send_to(&bytes, target_addr).await;
                    }

                    tokio::time::sleep(Duration::from_millis(500)).await;

                    let target_responded = {
                        let mut pending = pending_pings.lock().await;
                        !pending.remove(&target_id)
                    };

                    let indirect_ack = GossipMessage::IndirectAck {
                        from: local_info.0,
                        target_id,
                        target_responded,
                    };

                    if let Ok(bytes) = indirect_ack.to_bytes() {
                        if let Err(e) = socket.send_to(&bytes, from_addr).await {
                            debug!("Failed to send IndirectAck: {}", e);
                        } else {
                            debug!(
                                "Sent IndirectAck to {} - target responded: {}",
                                from.0, target_responded
                            );
                        }
                    }
                });
            }

            GossipMessage::IndirectAck {
                from,
                target_id,
                target_responded,
            } => {
                debug!(
                    "Handling IndirectAck from {}: target {} responded={}",
                    from.0, target_id.0, target_responded
                );

                let mut pending = self.pending_indirect_pings.lock().await;

                if let Some(state) = pending.get_mut(&target_id) {
                    state.responses.push(target_responded);

                    if target_responded {
                        info!(
                            "Target {} confirmed alive via indirect ping from {}",
                            target_id.0, from.0
                        );

                        let mut members = self.member_list.write().await;
                        members.mark_alive(&target_id);

                        let mut direct_pending = self.pending_pings.lock().await;
                        direct_pending.remove(&target_id);

                        pending.remove(&target_id);
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_member_updates(&self, updates: Vec<MemberUpdate>) {
        let mut members = self.member_list.write().await;
        let local_id = members.local_member().id.clone();

        for update in updates {
            if update.member_id == local_id && (update.state == MemberState::Suspect || update.state == MemberState::Dead) {
                warn!("Received false accusation - disputing.");
                members.increment_incarnation();
                continue;
            }
            
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
        pending_indirect_pings: Arc<Mutex<HashMap<MemberId, IndirectPingState>>>,
        backend_pool: SharedBackendPool,
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
                let mut pending_indirect = pending_indirect_pings.lock().await;
                let now = Instant::now();

                let timed_out: Vec<MemberId> = pending_indirect
                    .iter()
                    .filter(|(_, state)| {
                        now.duration_since(state.started_at) > Duration::from_secs(2)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();

                for member_id in timed_out {
                    if let Some(state) = pending_indirect.remove(&member_id) {
                        let any_success = state.responses.iter().any(|&r| r);

                        if !any_success {
                            warn!(
                                "All indirect pings failed for {} - marking as suspect",
                                member_id.0
                            );
                            let mut members = member_list.write().await;
                            members.mark_suspect(&member_id);
                        }
                    }
                }
            }

            {
                let mut pending = pending_pings.lock().await;
                let pending_indirect = pending_indirect_pings.lock().await;

                if !pending.is_empty() {
                    let members_to_indirect: Vec<Member> = {
                        let members = member_list.read().await;
                        pending
                            .iter()
                            .filter(|id| !pending_indirect.contains_key(id)) // Not already doing indirect
                            .filter_map(|id| {
                                members.get_all_members().into_iter().find(|m| &m.id == id)
                            })
                            .collect()
                    };

                    drop(pending_indirect); // Release lock before async operation

                    for target in members_to_indirect {
                        warn!("No direct ACK from {} - trying indirect pings", target.id.0);

                        if let Err(e) = GossipLayer::send_indirect_pings(
                            &member_list,
                            &socket,
                            &pending_indirect_pings,
                            target,
                            3, // Try 3 indirect probers
                        )
                        .await
                        {
                            error!("Error sending indirect pings: {}", e);
                        }
                    }

                    // Clear direct pending (we're now doing indirect)
                    pending.clear();
                }
            }

            let target = {
                let mut members = member_list.write().await;
                members.get_random_alive_member()
            };

            if let Some(target_member) = target {
                {
                    let mut pending = pending_pings.lock().await;
                    pending.insert(target_member.id.clone());
                }
                let local_info = {
                    let members = member_list.read().await;
                    let backends = backend_pool.lock().await;
                    let local = members.local_member();

                    let mut backend_updates = backends.get_backend_health_updates();
                    for update in &mut backend_updates {
                        update.from_member = local.id.clone();
                    }

                    (
                        local.id.clone(),
                        local.addr,
                        local.incarnation,
                        members.get_member_updates(5),
                        backend_updates,
                    )
                };

                let ping = GossipMessage::Ping {
                    from: local_info.0.clone(),
                    from_addr: local_info.1,
                    incarnation: local_info.2,
                    member_updates: local_info.3,
                    backend_updates: local_info.4,
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

    async fn process_backend_updates(&self, updates: Vec<BackendUpdate>) {
        if updates.is_empty() {
            return;
        }

        let mut backends = self.backend_pool.lock().await;
        for update in updates {
            backends.apply_backend_update(&update);
        }
    }

    async fn send_indirect_pings(
        member_list: &SharedMemberList,
        socket: &Arc<UdpSocket>,
        pending_indirect: &Arc<Mutex<HashMap<MemberId, IndirectPingState>>>,
        target: Member,
        num_indirect: usize,
    ) -> Result<()> {
        let indirect_probers: Vec<Member> = {
            let members = member_list.read().await;
            let local_id = members.local_member().id.clone();

            members
                .get_alive_members()
                .into_iter()
                .filter(|m| m.id != target.id && m.id != local_id) // Not target, not us
                .take(num_indirect)
                .collect()
        };

        if indirect_probers.is_empty() {
            warn!("No members available for indirect ping of {}", target.id.0);
            return Ok(());
        }

        info!(
            "Sending {} indirect ping requests for {} via {:?}",
            indirect_probers.len(),
            target.id.0,
            indirect_probers.iter().map(|m| &m.id.0).collect::<Vec<_>>()
        );

        {
            let mut pending = pending_indirect.lock().await;
            pending.insert(
                target.id.clone(),
                IndirectPingState {
                    target: target.clone(),
                    responses: Vec::new(),
                    started_at: Instant::now(),
                },
            );
        }

        let local_info = {
            let members = member_list.read().await;
            let local = members.local_member();
            (local.id.clone(), local.addr)
        };

        for prober in indirect_probers {
            let indirect_ping = GossipMessage::IndirectPing {
                from: local_info.0.clone(),
                from_addr: local_info.1,
                target_id: target.id.clone(),
                target_addr: target.addr,
            };

            if let Ok(bytes) = indirect_ping.to_bytes() {
                socket.send_to(&bytes, prober.addr).await?;
                debug!(
                    "Sent indirect ping request to {} for target {}",
                    prober.id.0, target.id.0
                );
            }
        }

        Ok(())
    }
}
