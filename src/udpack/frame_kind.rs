use bytes::Bytes;

/// Frame kind
#[derive(Debug)]
pub enum FrameKind {
  Open(bool),
  Ready,
  Ping,
  Pong,
  Data { pack_id: u32, bytes: Bytes },
  Ack { pack_id: u32 },
  Sync { pack_id: u32 },
  Slow { pack_id: u32 },
  Lost { pack_id: u32 },
  Shutdown,
  Close,
  Error,
}

impl FrameKind {
  pub const OPEN: u8 = 0x00;
  pub const READY: u8 = 0x01;
  pub const PING: u8 = 0x02;
  pub const PONG: u8 = 0x03;
  pub const DATA: u8 = 0x04;
  pub const ACK: u8 = 0x05;
  pub const SYNC: u8 = 0x06;
  pub const SLOW: u8 = 0x07;
  pub const LOST: u8 = 0x08;
  pub const SHUTDOWN: u8 = 0x09;
  pub const CLOSE: u8 = 0x0A;
  pub const ERROR: u8 = 0x0B;

  /// convert FrameKind to u8
  pub fn to(&self) -> u8 {
    match self {
      Self::Open(_) => Self::OPEN,
      Self::Ready => Self::READY,
      Self::Ping => Self::PING,
      Self::Pong => Self::PONG,
      Self::Data { .. } => Self::DATA,
      Self::Ack { .. } => Self::ACK,
      Self::Sync { .. } => Self::SYNC,
      Self::Slow { .. } => Self::SLOW,
      Self::Lost { .. } => Self::LOST,
      Self::Shutdown => Self::SHUTDOWN,
      Self::Close => Self::CLOSE,
      Self::Error => Self::ERROR,
    }
  }
}
