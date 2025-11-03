use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemberId(pub String);

impl MemberId {
    pub fn new(name: String) -> Self {
        Self(name)
    }

    pub fn generate(addr: SocketAddr) -> Self {
        Self(format!("flux-{}", addr))
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

const MAX_UDP_PACKET_SIZE: usize = 1400;

impl GossipMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let bytes = bincode::serialize(self)?;
        if bytes.len() > MAX_UDP_PACKET_SIZE {
            return Err(anyhow::anyhow!(
                "Message size {} exceeds max UDP packet size {}",
                bytes.len(),
                MAX_UDP_PACKET_SIZE
            ));
        }
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let message = bincode::deserialize(bytes)?;
        Ok(message)
    }

    pub fn estimated_size(&self) -> usize {
        bincode::serialized_size(self).unwrap_or(0) as usize
    }

    pub fn trim_to_fit(self) -> Self {
        match self {
            GossipMessage::Ping {
                from,
                from_addr,
                incarnation,
                mut member_updates,
                mut backend_updates,
            } => {
                let mut msg = GossipMessage::Ping {
                    from: from.clone(),
                    from_addr,
                    incarnation,
                    member_updates: vec![],
                    backend_updates: vec![],
                };

                while msg.estimated_size() < MAX_UDP_PACKET_SIZE && !member_updates.is_empty() {
                    if let Some(update) = member_updates.pop() {
                        if let GossipMessage::Ping { member_updates: ref mut updates, .. } = msg {
                            updates.push(update);
                        }
                    }
                }

                while msg.estimated_size() < MAX_UDP_PACKET_SIZE && !backend_updates.is_empty() {
                    if let Some(update) = backend_updates.pop() {
                        if let GossipMessage::Ping { backend_updates: ref mut updates, .. } = msg {
                            updates.push(update);
                        }
                    }
                }

                msg
            }
            GossipMessage::Ack {
                from,
                from_addr,
                incarnation,
                mut member_updates,
                mut backend_updates,
            } => {
                let mut msg = GossipMessage::Ack {
                    from: from.clone(),
                    from_addr,
                    incarnation,
                    member_updates: vec![],
                    backend_updates: vec![],
                };

                while msg.estimated_size() < MAX_UDP_PACKET_SIZE && !member_updates.is_empty() {
                    if let Some(update) = member_updates.pop() {
                        if let GossipMessage::Ack { member_updates: ref mut updates, .. } = msg {
                            updates.push(update);
                        }
                    }
                }

                while msg.estimated_size() < MAX_UDP_PACKET_SIZE && !backend_updates.is_empty() {
                    if let Some(update) = backend_updates.pop() {
                        if let GossipMessage::Ack { backend_updates: ref mut updates, .. } = msg {
                            updates.push(update);
                        }
                    }
                }

                msg
            }
            other => other,
        }
    }
}

