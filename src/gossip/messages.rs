use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

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

