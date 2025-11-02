use super::messages::{Member};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct IndirectPingState {
    pub(super) target: Member,
    pub(super) responses: Vec<bool>,
    pub(super) started_at: Instant,
}
