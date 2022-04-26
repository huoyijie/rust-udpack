use crate::udpack::{command::Command, macros};
use bytes::Bytes;
use std::{io, net::SocketAddr};
use tokio::sync::{
  mpsc::{UnboundedReceiver, UnboundedSender},
  oneshot,
};

/// Transport instance builder.
pub struct Builder;
impl Builder {
  /// Construct a Transport instance.
  pub fn new(uuid: u64, tx: UnboundedSender<Command>, rx2: UnboundedReceiver<Bytes>) -> Transport {
    Transport::new(uuid, tx, rx2)
  }
}

/// Represents a connection instance,
/// there are two ways to create it,
/// udpack.connect() or udpack.accept().
#[derive(Debug)]
pub struct Transport {
  // Unique ID of the connection, 16 bytes long.
  uuid: u64,
  // Command Sender
  tx: UnboundedSender<Command>,
  // Bytes Receiver for read()
  rx2: UnboundedReceiver<Bytes>,
}

impl Transport {
  /// return uuid of the connection.
  pub fn uuid(&self) -> u64 {
    self.uuid
  }

  /// return remote address of the connection.
  pub async fn remote_addr(&self) -> io::Result<SocketAddr> {
    let (s, r) = oneshot::channel();

    macros::tx_send!(
      self.tx,
      Command::GetRemoteAddr(s, self.uuid),
      "get remote addr failed (the rx dropped)"
    );

    macros::r!(r, "get remote addr failed (the s dropped)")
  }

  /// ## Examples
  /// ```
  /// #[tokio::main]
  /// async fn main() -> io::Result<()> {
  ///   let udpack: Udpack = Udpack::new("0.0.0.0:0").await?;
  ///   let mut transport: Transport = udpack.connect("127.0.0.1:8080").await?;
  ///   let mut interval = time::interval(Duration::from_secs(3));
  ///
  ///   loop {
  ///     tokio::select! {
  ///       res = transport.read() => {
  ///         if let Some(bytes) = res {
  ///           println!("{:?}", bytes);
  ///         }
  ///       }
  ///       _ = interval.tick() => {
  ///         transport.write(Bytes::copy_from_slice(&[1u8; 2048])).await?;
  ///       }
  ///       _ = signal::ctrl_c() => {
  ///         println!("ctrl-c received!");
  ///         udpack.shutdown().await?;
  ///         return Ok(());
  ///       }
  ///     };
  ///   }
  /// }
  /// ```
  /// This method is cancel safe.
  ///
  /// Returning None means that the other peer has shutdown the connection.
  pub async fn read(&mut self) -> Option<Bytes> {
    match self.rx2.recv().await {
      Some(bytes) if bytes.len() > 0 => Some(bytes),
      _ => None,
    }
  }

  /// ## Examples
  /// ```let writable = transport.writable().await?; ```
  ///
  /// Returns whether writable or not on success, or an error on failure.
  /// Writable only means that the write buffer doesn't exceed the threshold,
  /// therefore the write call isn't rejected even if writable is false.
  /// It is always better to call this method before write.
  pub async fn writable(&self) -> io::Result<bool> {
    let (s, r) = oneshot::channel();

    macros::tx_send!(
      self.tx,
      Command::Writable(s, self.uuid),
      "writable failed (the rx dropped)"
    );

    macros::r!(r, "writable failed (the s dropped)")
  }

  /// ## Examples
  /// ``` transport.write(Bytes::copy_from_slice(&[1u8; 2048])).await?; ```
  ///
  /// Returns the number of bytes on success, or an error on failure.
  pub async fn write(&self, bytes: Bytes) -> io::Result<usize> {
    let (s, r) = oneshot::channel();

    macros::tx_send!(
      self.tx,
      Command::Write(s, self.uuid, bytes),
      "write failed (the rx dropped)"
    );

    macros::r!(r, "write failed (the s dropped)")
  }

  /// ## Examples
  /// ``` transport.ping()?; ```
  ///
  /// Send a ping frame, and the other peer will reply a pong frame immediately.
  pub fn ping(&self) -> io::Result<()> {
    macros::tx_send!(
      self.tx,
      Command::Ping(self.uuid),
      "ping failed (the rx dropped)"
    );
    Ok(())
  }

  /// ## Examples
  /// ``` transport.shutdown(); ```
  ///
  /// Transport implements Drop, it will be called automatically when dropped.
  /// In general, no manual call is required.
  pub fn shutdown(&self) -> io::Result<()> {
    macros::tx_send!(
      self.tx,
      Command::Shutdown(self.uuid),
      "shutdown failed (the rx dropped)"
    );
    Ok(())
  }

  /// Close the connection rudely without flushing the reading and writing buffer.
  /// The data in the buffer will be discarded.
  pub fn close(&self) -> io::Result<()> {
    macros::tx_send!(
      self.tx,
      Command::Close(self.uuid),
      "close failed (the rx dropped)"
    );
    Ok(())
  }

  // Construct a Transport instance.
  fn new(uuid: u64, tx: UnboundedSender<Command>, rx2: UnboundedReceiver<Bytes>) -> Self {
    Self { uuid, tx, rx2 }
  }
}

impl Drop for Transport {
  fn drop(&mut self) {
    macros::ignore_err!(self.shutdown());
  }
}
