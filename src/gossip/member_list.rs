use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::messages::{Member, MemberId, MemberState, MemberUpdate};

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
