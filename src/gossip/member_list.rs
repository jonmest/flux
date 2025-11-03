use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use std::time::{SystemTime, UNIX_EPOCH};
use super::messages::{Member, MemberId, MemberState, MemberUpdate};

fn simple_rand(seed: &mut u64) -> u64 {
    // Linear Congruential Generator (LCG): not great RNG, but fine for quick shuffling
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (*seed >> 33) ^ *seed
}

fn shuffle<T>(v: &mut [T]) {
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut seed = start as u64;

    for i in (1..v.len()).rev() {
        let j = (simple_rand(&mut seed) as usize) % (i + 1);
        v.swap(i, j);
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
    order: Vec<MemberId>,
    index: HashMap<MemberId, u64>,
    suspect_timeout: Duration,
    cursor: usize,
}

impl MemberList {
    pub fn new(local_member: Member, suspect_timeout: Duration) -> Self {
        let mut members = HashMap::new();
        let mut order = Vec::new();
        let mut index = HashMap::new();

        // add ourselves to the member list
        members.insert(
            local_member.id.clone(),
            MemberInfo::new(local_member.clone()),
        );
        order.push(local_member.id.clone());
        index.insert(local_member.id.clone(), 0);

        Self {
            local_member,
            members,
            order,
            index,
            suspect_timeout,
            cursor: 0,
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

            self.members
                .insert(member_id.clone(), MemberInfo::new(member));
            // TODO
            self.order.push(member_id.clone());
            self.index
                .insert(member_id, (self.order.len() - 1).try_into().unwrap());

            shuffle(&mut self.order);
            for (index, mid) in self.order.iter().enumerate() {
                self.index.insert(mid.clone(), index.try_into().unwrap());
            }

            self.cursor = 0;
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
        self.order.iter()
            .map(|id| self.members.get(id))
            .filter(|item| item.is_some())
            .map(|i| i.unwrap())
            .filter(|info| {
                info.member.state == MemberState::Alive && info.member.id != self.local_member.id
            })
            .map(|info| info.member.clone())
            .collect()

        // self.members
        //     .values()
        //     .filter(|info| {
        //         info.member.state == MemberState::Alive && info.member.id != self.local_member.id
        //     })
        //     .map(|info| info.member.clone())
        //     .collect()
    }

    pub fn get_all_members(&self) -> Vec<Member> {
        self.order.iter()
            .map(|id| self.members.get(id))
            .filter(|item| item.is_some())
            .map(|i| i.unwrap())
            .filter(|m| m.member.id != self.local_member.id)
            .map(|m| m.member.clone())
            .collect()
    }

    pub fn get_random_alive_member(&mut self) -> Option<Member> {
        if self.order.is_empty() {
            return None;
        }

        for _ in 0..self.order.len() {
            let member_id = &self.order[self.cursor % self.order.len()];
            self.cursor += 1;

            if let Some(info) = self.members.get(member_id) {
                if info.member.state == MemberState::Alive
                    && info.member.id != self.local_member.id
                {
                    return Some(info.member.clone());
                }
            }
        }

        None
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

        let mut pruned_ids = Vec::new();

        self.members.retain(|id, info| {
            if info.member.state == MemberState::Dead {
                let time_dead = now.duration_since(info.last_seen);
                if time_dead > dead_timeout {
                    info!("Pruning dead member: {}", id.0);
                    pruned_ids.push(id.clone());
                    return false;
                }
            }
            true
        });

        self.order.retain(|id| !pruned_ids.contains(id));

        shuffle(&mut self.order);
        self.index.clear();
        for (index, mid) in self.order.iter().enumerate() {
            self.index.insert(mid.clone(), index.try_into().unwrap());
        }

        // Reset cursor after reshuffling
        self.cursor = 0;
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
