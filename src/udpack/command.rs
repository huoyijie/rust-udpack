use crate::transport::Transport;
use bytes::Bytes;
use std::io;
use std::net::SocketAddr;
use tokio::sync::oneshot;

pub enum Command {
  // udpack.connect
  Connect(oneshot::Sender<io::Result<Transport>>, String),
  // udpack.shutdown
  ShutdownAll,

  // transport.writable
  Writable(oneshot::Sender<io::Result<bool>>, u64),
  // transport.write
  Write(oneshot::Sender<io::Result<usize>>, u64, Bytes),
  // transport.ping
  Ping(u64),
  // transport.remote_addr
  GetRemoteAddr(oneshot::Sender<io::Result<SocketAddr>>, u64),
  // transport.shutdown
  Shutdown(u64),
  // transport.close
  Close(u64),
}
