use super::frame_kind::FrameKind;

/// Frame has two parts, one part is fixed with 10
/// bytes, and the other one is optional.
/// ```
/// +--------------------+-----------------+
/// |      required      |    optional     |
/// +--------------------+-------+---------+
/// |           header           |  body   |
/// +------+------+------+-------+---------+
/// | uuid | kind | flag |pack id|pack data|
/// +------+------+------+-------+---------+
/// |  8   |  1   |  1   |   4   |   len   |
/// +------+------+------+-------+---------+
/// ```
/// uuid: transport uuid
#[derive(Debug)]
pub struct Frame {
  uuid: u64,
  kind: FrameKind,
}

impl Frame {
  pub const UUID_LEN: usize = 8;
  pub const HEADER_FIXED_LEN: usize = Self::UUID_LEN + 2;
  const HEADER_PACK_ID_LEN: usize = 4;

  pub const MAX_FRAME_LEN: usize = 1024;
  pub const MAX_HEADER_LEN: usize = Self::HEADER_FIXED_LEN + Self::HEADER_PACK_ID_LEN;
  pub const MAX_DATA_LEN: usize = Self::MAX_FRAME_LEN - Self::MAX_HEADER_LEN;

  pub fn new(uuid: u64, kind: FrameKind) -> Self {
    Self { uuid, kind }
  }

  pub fn uuid(&self) -> &u64 {
    &self.uuid
  }

  pub fn kind(&self) -> &FrameKind {
    &self.kind
  }
}
