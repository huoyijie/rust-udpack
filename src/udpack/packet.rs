use super::pack_state::PackState;
use bytes::Bytes;
use std::cmp::Ordering;
use tokio::time::Duration;

const SEND_PACK_TIMEOUT_MILLIS: u64 = 500;

/// Sending or receiving entities in the send/recv buffer
#[derive(Debug)]
pub struct Packet {
  /// Unique Id of the packet
  pack_id: u32,
  /// State of the packet
  state: PackState,
  /// Bytes sent or received
  bytes: Bytes,
  /// retry count of the packet
  retries: u32,
}

impl Packet {
  pub fn new(pack_id: u32, bytes: Bytes) -> Self {
    Self {
      pack_id,
      state: PackState::Init,
      bytes,
      retries: 0,
    }
  }

  pub fn pack_id(&self) -> u32 {
    self.pack_id
  }

  pub fn set_state(&mut self, state: PackState) -> &mut Self {
    if let PackState::Retry = state {
      self.retries += 1;
    }
    self.state = state;
    self
  }

  /// Whether the packet was timeout
  pub fn timeout(&self) -> bool {
    if let PackState::Sent(sent_time) = self.state {
      if sent_time.elapsed() > Duration::from_millis(SEND_PACK_TIMEOUT_MILLIS) {
        return true;
      }
    }
    return false;
  }

  /// Whether the packet was lost
  pub fn lost(&self) -> bool {
    if let PackState::Lost = self.state {
      return true;
    }
    return false;
  }

  pub fn bytes(&self) -> Bytes {
    self.bytes.clone()
  }

  pub fn retries(&self) -> u32 {
    self.retries
  }
}

impl Ord for Packet {
  fn cmp(&self, other: &Self) -> Ordering {
    self.pack_id.cmp(&other.pack_id)
  }
}

impl PartialOrd for Packet {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl PartialEq for Packet {
  fn eq(&self, other: &Self) -> bool {
    self.pack_id == other.pack_id
  }
}

impl Eq for Packet {}
