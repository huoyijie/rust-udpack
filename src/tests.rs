use crate::udpack::codec::FrameCodec;
use crate::udpack::frame::Frame;
use crate::udpack::frame_kind::FrameKind;
use crate::udpack::frame_task::FrameTask;
use crate::udpack::macros;
use crate::Transport;
use crate::Udpack;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use sonyflake::Sonyflake;
use std::io;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tokio_util::codec::Decoder;

#[tokio::test]
async fn udpack_new_and_shutdown() -> io::Result<()> {
  let udpack = Udpack::new("0.0.0.0:8080").await?;
  udpack.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn udpack_connect_accept() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();

  t1.shutdown()?;
  t2.shutdown()?;

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn udpack_force_shutdown_all() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let _t1 = client.connect("127.0.0.1:8080").await?;
  let _t2 = server.accept().await.unwrap();

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_new_shutdown() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();

  let n = t1.write(Bytes::copy_from_slice(&[1; 2048])).await?;
  assert!(n == 2048);
  t1.shutdown()?;

  let n = t2.write(Bytes::copy_from_slice(&[1u8; 10])).await?;
  assert!(n == 10);
  t2.shutdown()?;

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_close() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();

  t1.close()?;

  let res = t1.write(Bytes::copy_from_slice(&[1; 2048])).await;
  assert_eq!(res.is_err(), true);

  sleep(Duration::from_millis(100)).await;

  let res = t2.write(Bytes::copy_from_slice(&[1; 2048])).await;
  assert_eq!(res.is_err(), true);

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_dropped() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();
  drop(t1);

  sleep(Duration::from_millis(100)).await;

  let res = t2.write(Bytes::copy_from_slice(&[1; 2048])).await;
  assert_eq!(res.is_err(), false);
  t2.shutdown()?;

  let res = t2.write(Bytes::copy_from_slice(&[1; 2048])).await;
  assert_eq!(res.is_err(), true);

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_get_remote_addr() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:8081").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();

  assert_eq!(t1.remote_addr().await?.to_string(), "127.0.0.1:8080");
  assert_eq!(t2.remote_addr().await?.to_string(), "127.0.0.1:8081");

  t1.shutdown()?;
  t2.shutdown()?;

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_write_read() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let mut t2 = server.accept().await.unwrap();

  let n = t1.write(Bytes::copy_from_slice(&[1; 2048])).await?;
  assert!(n == 2048);
  drop(t1);

  let mut m = 0;
  while let Some(bytes) = t2.read().await {
    m += bytes.len();
  }
  assert!(m == 2048);
  drop(t2);

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[tokio::test]
async fn transport_ping() -> io::Result<()> {
  let client: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut server: Udpack = Udpack::new("0.0.0.0:8080").await?;
  let t1 = client.connect("127.0.0.1:8080").await?;
  let t2 = server.accept().await.unwrap();

  t1.ping()?;
  t2.ping()?;

  t1.shutdown()?;
  t2.shutdown()?;

  client.shutdown().await?;
  server.shutdown().await?;
  Ok(())
}

#[macro_use]
mod task_macro {
  #[macro_export]
  macro_rules! recv_frame {
    ($client: ident, $buf: ident, $src: ident, $codec: ident, $frame: ident => $done: block) => {
      let (n, _) = $client.recv_from(&mut $buf).await?;
      $src.put_slice(&$buf[..n]);
      let $frame = $codec.decode(&mut $src)?.unwrap();
      $done
    };
  }

  #[macro_export]
  macro_rules! connect {
    ($codec: ident, $dst: ident, $client: ident, $dst_addr: ident, $src: ident, $buf: ident, $uuid: ident) => {{
      // write Open(true)
      macros::write_frame!(
        $codec,
        $dst,
        Frame::new($uuid, FrameKind::Open(true)),
        $client,
        $dst_addr
      );

      // received Open(false)
      recv_frame!($client, $buf, $src, $codec, frame => {
        assert_eq!(*frame.uuid(), $uuid);
        if let FrameKind::Open(init) = frame.kind() {
          assert_eq!(*init, false);
        } else {
          panic!("should receive Open(false) frame");
        }
      });
    }

    {
      // write Ready frame
      macros::write_frame!(
        $codec,
        $dst,
        Frame::new($uuid, FrameKind::Ready),
        $client,
        $dst_addr
      );
    }};
  }

  macro_rules! ping {
    ($codec: ident, $dst: ident, $client: ident, $dst_addr: ident, $src: ident, $buf: ident, $uuid: ident) => {{
      // write Ping frame
      macros::write_frame!(
        $codec,
        $dst,
        Frame::new($uuid, FrameKind::Ping),
        $client,
        $dst_addr
      );

      // received Pong frame
      recv_frame!($client, $buf, $src, $codec, frame => {
        assert_eq!(*frame.uuid(), $uuid);
        assert_eq!(frame.kind().to(), FrameKind::PONG);
      });
    }};
  }
}

async fn frame_task_prepare_data() -> io::Result<(
  FrameCodec,
  BytesMut,
  BytesMut,
  [u8; Frame::MAX_FRAME_LEN],
  u64,
  UdpSocket,
  UnboundedReceiver<Transport>,
)> {
  let socket = UdpSocket::bind("0.0.0.0:8080").await?;
  let (tx, rx) = mpsc::unbounded_channel();
  let (tx1, rx1) = mpsc::unbounded_channel();
  let mut frame_task = FrameTask::new(socket, tx, rx, tx1);
  let _handle: JoinHandle<io::Result<()>> = tokio::spawn(async move { frame_task.execute().await });

  let codec = FrameCodec;
  let dst = BytesMut::with_capacity(Frame::MAX_FRAME_LEN);
  let src = BytesMut::with_capacity(Frame::MAX_FRAME_LEN);
  let buf = [0u8; Frame::MAX_FRAME_LEN];

  let mut sf = Sonyflake::new().unwrap();
  let uuid = sf.next_id().unwrap();
  let client = UdpSocket::bind("0.0.0.0:0").await?;
  Ok((codec, dst, src, buf, uuid, client, rx1))
}

#[tokio::test]
async fn frame_task_ping_without_session() -> io::Result<()> {
  let (mut codec, mut dst, mut src, mut buf, uuid, client, _rx1) =
    frame_task_prepare_data().await?;
  let dst_addr = "127.0.0.1:8080";

  // write Ping frame
  macros::write_frame!(
    codec,
    dst,
    Frame::new(uuid, FrameKind::Ping),
    client,
    dst_addr
  );

  // read Close frame
  recv_frame!(client, buf, src, codec, frame => {
    assert_eq!(frame.kind().to(), FrameKind::ERROR);
  });

  Ok(())
}

#[tokio::test]
async fn frame_task_setup_session_and_ping() -> io::Result<()> {
  let (mut codec, mut dst, mut src, mut buf, uuid, client, _rx1) =
    frame_task_prepare_data().await?;
  let dst_addr = "127.0.0.1:8080";

  connect!(codec, dst, client, dst_addr, src, buf, uuid);

  ping!(codec, dst, client, dst_addr, src, buf, uuid);

  Ok(())
}

#[tokio::test]
async fn frame_task_send_bytes_ack_and_sync() -> io::Result<()> {
  let (mut codec, mut dst, mut src, mut buf, uuid, client, _rx1) =
    frame_task_prepare_data().await?;
  let dst_addr = "127.0.0.1:8080";

  connect!(codec, dst, client, dst_addr, src, buf, uuid);

  let pack_id = 1;
  let mut bytes = BytesMut::with_capacity(10);
  bytes.put_bytes(255, 10);
  let bytes = bytes.freeze();

  // write Data frame
  macros::write_frame!(
    codec,
    dst,
    Frame::new(uuid, FrameKind::Data { pack_id, bytes }),
    client,
    dst_addr
  );

  // read Ack frame
  recv_frame!(client, buf, src, codec, frame => {
    assert_eq!(frame.kind().to(), FrameKind::ACK);
    if let FrameKind::Ack { pack_id } = frame.kind() {
      assert_eq!(*pack_id, 1);
    }
  });

  // read Sync frame
  recv_frame!(client, buf, src, codec, frame => {
    assert_eq!(frame.kind().to(), FrameKind::SYNC);
    if let FrameKind::Sync { pack_id } = frame.kind() {
      assert_eq!(*pack_id, 2);
    }
  });

  Ok(())
}

#[tokio::test]
async fn frame_task_send_bytes_lost() -> io::Result<()> {
  let (mut codec, mut dst, mut src, mut buf, uuid, client, _rx1) =
    frame_task_prepare_data().await?;
  let dst_addr = "127.0.0.1:8080";

  connect!(codec, dst, client, dst_addr, src, buf, uuid);

  let pack_id = 2;
  let mut bytes = BytesMut::with_capacity(10);
  bytes.put_bytes(255, 10);
  let bytes = bytes.freeze();

  // write Data frame
  macros::write_frame!(
    codec,
    dst,
    Frame::new(uuid, FrameKind::Data { pack_id, bytes }),
    client,
    dst_addr
  );

  // read Ack frame
  recv_frame!(client, buf, src, codec, frame => {
    assert_eq!(frame.kind().to(), FrameKind::ACK);
    if let FrameKind::Ack { pack_id } = frame.kind() {
      assert_eq!(*pack_id, 2);
    }
  });

  // read Lost frame
  recv_frame!(client, buf, src, codec, frame => {
    assert_eq!(frame.kind().to(), FrameKind::LOST);
    if let FrameKind::Lost { pack_id } = frame.kind() {
      assert_eq!(*pack_id, 1);
    }
  });
  Ok(())
}
