use super::{
  frame::Frame,
  frame_kind::{FrameKind, FrameKind::*},
};
use bytes::{Buf, BufMut, BytesMut};
use std::{io, io::ErrorKind};
use tokio_util::codec::{Decoder, Encoder};

/// Frame encoder and decoder
#[derive(Debug)]
pub struct FrameCodec;

impl FrameCodec {
  fn put_fixed_header(&self, additional: usize, item: &Frame, dst: &mut BytesMut) {
    dst.reserve(additional);
    dst.put_u64(*item.uuid());
    dst.put_u8(item.kind().to());
    let flag: u8 = match item.kind() {
      Open(true) => 0x01,
      _ => 0x00,
    };
    dst.put_u8(flag);
  }

  fn check_pack_id(&self, pack_id: u32) -> Result<(), <Self as Encoder<Frame>>::Error> {
    if pack_id == 0 {
      Err(<Self as Encoder<Frame>>::Error::new(
        ErrorKind::InvalidData,
        format!("pack_id = 0"),
      ))
    } else {
      Ok(())
    }
  }
}

impl Encoder<Frame> for FrameCodec {
  type Error = std::io::Error;

  fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
    match item.kind() {
      Open(_) | Ready | Ping | Pong | Shutdown | Close | Error => {
        self.put_fixed_header(Frame::HEADER_FIXED_LEN, &item, dst);
      }
      Data { pack_id, bytes } => {
        self.check_pack_id(*pack_id)?;
        let body_len = bytes.len();
        if body_len > Frame::MAX_DATA_LEN {
          return Err(Self::Error::new(
            ErrorKind::InvalidData,
            format!("Frame of bytes length {} is too large.", body_len),
          ));
        }

        self.put_fixed_header(Frame::MAX_HEADER_LEN + body_len, &item, dst);
        dst.put_u32(*pack_id);
        dst.put_slice(bytes);
      }
      Ack { pack_id } | Sync { pack_id } | Slow { pack_id } | Lost { pack_id } => {
        self.check_pack_id(*pack_id)?;
        self.put_fixed_header(Frame::MAX_HEADER_LEN, &item, dst);
        dst.put_u32(*pack_id);
      }
    };
    Ok(())
  }
}

impl Decoder for FrameCodec {
  type Item = Frame;
  type Error = std::io::Error;
  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    let uuid = src.get_u64();
    let kind = src.get_u8();
    let flag = src.get_u8();
    match kind {
      FrameKind::OPEN => Ok(Some(Frame::new(uuid, Open(flag == 0x01)))),
      FrameKind::READY => Ok(Some(Frame::new(uuid, Ready))),
      FrameKind::PING => Ok(Some(Frame::new(uuid, Ping))),
      FrameKind::PONG => Ok(Some(Frame::new(uuid, Pong))),
      FrameKind::DATA => {
        let pack_id = src.get_u32();
        Ok(Some(Frame::new(
          uuid,
          Data {
            pack_id,
            bytes: src.copy_to_bytes(src.len()),
          },
        )))
      }
      FrameKind::ACK => Ok(Some(Frame::new(
        uuid,
        Ack {
          pack_id: src.get_u32(),
        },
      ))),
      FrameKind::SYNC => Ok(Some(Frame::new(
        uuid,
        Sync {
          pack_id: src.get_u32(),
        },
      ))),
      FrameKind::SLOW => Ok(Some(Frame::new(
        uuid,
        Slow {
          pack_id: src.get_u32(),
        },
      ))),
      FrameKind::LOST => Ok(Some(Frame::new(
        uuid,
        Lost {
          pack_id: src.get_u32(),
        },
      ))),
      FrameKind::SHUTDOWN => Ok(Some(Frame::new(uuid, Shutdown))),
      FrameKind::CLOSE => Ok(Some(Frame::new(uuid, Close))),
      FrameKind::ERROR => Ok(Some(Frame::new(uuid, Error))),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Frame of kind {} is invalid.", kind),
      )),
    }
  }
}
