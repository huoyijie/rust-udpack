pub mod codec;
pub mod command;
pub mod frame;
pub mod frame_kind;
pub mod frame_task;
pub mod macros;
pub mod pack_state;
pub mod packet;
pub mod session;
pub mod speed;
pub mod state;

use crate::udpack::codec::FrameCodec;
use crate::udpack::command::Command;
use crate::udpack::frame::Frame;
use crate::udpack::frame_kind::FrameKind;
use crate::udpack::frame_task::FrameTask;
use crate::udpack::pack_state::PackState;
use crate::udpack::packet::Packet;
use crate::udpack::session::Session;
use crate::udpack::state::State;
use crate::Transport;
use std::io;
use tokio::net::ToSocketAddrs;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// It is the main implementation class of UDPack, a connection-oriented reliable data transmission protocol based on udp.
#[derive(Debug)]
pub struct Udpack {
  // handle of frame codec task
  frame_codec_handle: JoinHandle<io::Result<()>>,
  // Command Sender
  tx: UnboundedSender<Command>,
  // Transport Receiver for accept()
  rx1: UnboundedReceiver<Transport>,
}

impl Udpack {
  /// Constructs an instance with the provided bind address.The address is used to construct a UdpSocket instance, if it fails, it returns an error message, and if it succeeds, it returns a Udpack instance.
  /// ## Examples
  /// ``` let udpack: Udpack = Udpack::new("0.0.0.0:0").await?;```
  ///
  /// If the port number is 0, it will be assigned randomly.
  pub async fn new<A: ToSocketAddrs>(bind_addr: A) -> io::Result<Self> {
    let socket = UdpSocket::bind(bind_addr).await?;
    socket.set_ttl(255)?;
    let (tx, rx) = unbounded_channel();
    let (tx1, rx1) = unbounded_channel();

    let tx_clone = tx.clone();
    // start frame codec task for encoding and decoding frames
    let frame_codec_handle = tokio::spawn(async move {
      let task_result = FrameTask::new(socket, tx_clone, rx, tx1).execute().await;
      if task_result.is_err() {
        eprintln!("frame task result = {:?}", task_result);
      }
      return task_result;
    });

    Ok(Self {
      frame_codec_handle,
      tx,
      rx1,
    })
  }

  /// Establish a connection to the udpack instance at the supplied address,returns an error on failure, and a Transport instance on success.
  /// ## Examples
  /// ``` let mut transport: Transport = udpack.connect("127.0.0.1:8080").await?;```
  ///
  /// The connect timeout is 1500 milli seconds.
  pub async fn connect(&self, dst_addr: &str) -> io::Result<Transport> {
    let (s, r) = oneshot::channel();

    macros::tx_send!(
      self.tx,
      Command::Connect(s, dst_addr.to_string()),
      "connect failed (the rx dropped)"
    );

    macros::r!(r, "connect failed (the s dropped)")
  }

  /// Accepts a new connection, returns a Transport instance if successful, or None if failed.
  /// This method is cancel safe.
  /// ## Examples
  /// ```
  /// #[tokio::main]
  /// async fn main() -> io::Result<()> {
  ///   let mut udpack: Udpack = Udpack::new("0.0.0.0:8080").await?;
  ///
  ///   loop {
  ///     tokio::select! {
  ///       res = udpack.accept() => {
  ///         let _handle: JoinHandle<io::Result<()>> = tokio::spawn(async move {
  ///           let mut transport: Transport = res.unwrap();
  ///
  ///           while let Some(bytes) = transport.read().await {
  ///             println!("{:?}", bytes);
  ///             transport.write(bytes).await?;
  ///           }
  ///           Ok(())
  ///         });
  ///       }
  ///       _ = signal::ctrl_c() => {
  ///         println!("ctrl-c received!");
  ///         udpack.shutdown().await?;
  ///         break;
  ///       }
  ///     }
  ///   }
  ///   Ok(())
  /// }
  /// ```
  pub async fn accept(&mut self) -> Option<Transport> {
    self.rx1.recv().await
  }

  /// Stop accepting new connections and notify all existing connections to exit step by step. All connections will no longer accept new write data requests before exiting, but will try their best to ensure that all data in the buffer is sent. And will continue to respond to read requests until all data is read.
  /// ## Examples
  /// ``` udpack.shutdown().await? ```
  ///
  /// It should be callde before exiting.
  /// The shutdown timeout is 3000 milli seconds.
  pub async fn shutdown(self) -> io::Result<()> {
    macros::tx_send!(
      self.tx,
      Command::ShutdownAll,
      "shutdown all failed (the rx dropped)"
    );

    self.closed().await
  }

  // waiting for the frame codec task to quit
  async fn closed(self) -> io::Result<()> {
    self.frame_codec_handle.await?
  }
}
