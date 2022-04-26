use crate::{
  transport::Builder,
  udpack::{
    macros, speed::Speed, state::State, Command, Frame, FrameCodec, FrameKind, FrameKind::*,
    PackState, Packet, Session,
  },
  Transport,
};
use bytes::{Buf, BufMut, BytesMut};
use rand::Rng;
use sonyflake::Sonyflake;
use std::{collections::HashMap, error::Error, io, marker, net::SocketAddr};
use tokio::{
  net::UdpSocket,
  sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
  },
  time::{interval, Duration, Instant, Interval},
};
use tokio_util::codec::Decoder;

pub const CONNECT_TIMEOUT_MILLIS: u64 = 1500;
const SHUTDOWN_TIMEOUT_MILLIS: u64 = 3000;
const RETRY_SEND_MAX: u16 = 256;
const TASK_INTERVAL_MILLIS: u64 = 10;
const SPEED_INTERVAL_SECS: u64 = 1;

fn machine_id() -> Result<u16, Box<dyn Error + 'static + Send + marker::Sync>> {
  Ok(rand::thread_rng().gen())
}

pub struct FrameTask {
  // udp socket
  socket: UdpSocket,

  // command channel sender
  tx: UnboundedSender<Command>,

  // command channel receiver
  rx: UnboundedReceiver<Command>,

  // means if called udpack.shutdown() or udpack dropped
  shutdown_all: Option<Instant>,

  // store sessions of transports
  session_map: HashMap<u64, Session>,

  // store connect request
  connect_req_map: HashMap<u64, (oneshot::Sender<io::Result<Transport>>, Instant)>,

  // transport sender for udpack.accept()
  tx1: UnboundedSender<Transport>,

  // task interval
  interval: Interval,

  // encoder and decoder of frames
  codec: FrameCodec,

  // encoder buffer
  dst: BytesMut,

  // decoder buffer
  src: BytesMut,

  // buf for socket.recv_from
  buf: [u8; Frame::MAX_FRAME_LEN],

  // speed interval
  speed_interval: Interval,

  // send speed
  send_speed: Speed,

  // recv speed
  recv_speed: Speed,

  // sonyflake
  sf: Sonyflake,
}

impl FrameTask {
  pub fn new(
    socket: UdpSocket,
    tx: UnboundedSender<Command>,
    rx: UnboundedReceiver<Command>,
    tx1: UnboundedSender<Transport>,
  ) -> Self {
    Self {
      socket,
      tx,
      rx,
      tx1,
      shutdown_all: None,

      session_map: HashMap::new(),

      connect_req_map: HashMap::new(),

      interval: interval(Duration::from_millis(TASK_INTERVAL_MILLIS)),

      codec: FrameCodec,

      dst: BytesMut::with_capacity(Frame::MAX_FRAME_LEN),

      src: BytesMut::with_capacity(Frame::MAX_FRAME_LEN),

      buf: [0u8; Frame::MAX_FRAME_LEN],

      speed_interval: interval(Duration::from_secs(SPEED_INTERVAL_SECS)),

      send_speed: Speed::new(),

      recv_speed: Speed::new(),

      sf: Sonyflake::builder()
        .machine_id(&machine_id)
        .finalize()
        .unwrap(),
    }
  }

  pub async fn execute(&mut self) -> io::Result<()> {
    // loop until udpack shutdown or some error happens
    loop {
      tokio::select! {
        // read bytes and decode to frames
        res = self.socket.recv_from(&mut self.buf) => {
          self.frame_protocol(res).await?;
        }

        // receive commands sent from udpack or transports
        res = self.rx.recv() => {
          self.recv_command(res).await?;
        }

        // task interval
        _ = self.interval.tick() => {

          self.check_expired_sessions();

          self.check_timeout_connects();

          macros::retry_lost_packets!(
            self.socket,
            self.session_map
          );

          macros::retry_timeout_packets!(
            self.socket,
            self.session_map,
            RETRY_SEND_MAX
          );

          self.send_packets().await?;

          self.process_read_requests().await?;

          self.check_shutting_down().await?;

          let res = self.check_shutdown_all_finished();
          if !res.is_none() {
            return res.unwrap();
          }
        }

        // speed interval
        _ = self.speed_interval.tick() => {
          self.retry_ratio_ctrl();

          println!("[speed: send {}, recv {}]", self.send_speed.in_kilo_bytes(), self.recv_speed.in_kilo_bytes());
          self.send_speed.reset();
          self.recv_speed.reset();
        }
      }
    }
  }

  async fn frame_protocol(&mut self, res: io::Result<(usize, SocketAddr)>) -> io::Result<()> {
    // read data into buffer of decoder
    let (bytes_read, remote_addr) = res?;

    // transfer bytes into src for decode
    self.src.put_slice(&self.buf[..bytes_read]);

    // decode bytes to frames
    match self.codec.decode(&mut self.src) {
      // docode succeed
      Ok(Some(frame)) => {
        // update remote addr and last access time of the session
        macros::get_session!(
          self.session_map,
          frame.uuid(),
          session => {
            session
              .update_remote_addr(remote_addr)
              .update_last_acc_time();
          }
        );

        // process different kind of frame
        match frame.kind() {
          // Open frame
          // if the frame is sent from the initiator,
          // then initiator is true, otherwise false.
          Open(initiator) => {
            // create session
            let mut session = Session::new(State::Open(Instant::now()), remote_addr);

            // calculate the kind of the reply frame.
            let reply_kind = match *initiator {
              true => Open(false),
              false => {
                // initiator change session state to Ready.
                session.update_state(State::Ready);

                // get the connect request
                if let Some((s, _)) = self.connect_req_map.remove(frame.uuid()) {
                  let transport =
                    Builder::new(*frame.uuid(), self.tx.clone(), session.rx2().unwrap());
                  // send the transport to the task in which connect was called.
                  macros::s_send_transport!(s, transport);
                } else {
                  // not possible
                  return macros::io_interrupted_err!(format!(
                    "connect request of transport {} not exist.",
                    frame.uuid()
                  ));
                }
                FrameKind::Ready
              }
            };

            // insert session into the map.
            self.session_map.insert(*frame.uuid(), session);

            macros::write_frame!(
              self.codec,
              self.dst,
              Frame::new(*frame.uuid(), reply_kind),
              self.socket,
              remote_addr
            );
          }

          // Ready frame
          FrameKind::Ready => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                // update session state to Ready
                session.update_state(State::Ready);

                // new transport
                let transport = Builder::new(
                  *frame.uuid(),
                  self.tx.clone(),
                  session.rx2().unwrap()
                );

                // send transport to udpack.accept()
                macros::tx_send!(self.tx1, transport, "send transport failed; err = the rx1 dropped");
              }
            );
          }

          // Ping frame
          Ping => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              _session => {
                // received Ping, send Pong
                macros::write_frame!(
                  self.codec,
                  self.dst,
                  Frame::new(*frame.uuid(), Pong),
                  self.socket,
                  remote_addr
                );
              }
            );
          }

          // Pong frame
          Pong => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              _session => {}
            );
          }

          // Data frame
          Data { pack_id, bytes } => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                let pack_id = *pack_id;

                // possible duplicate reception
                if !session.pack_received(pack_id) {
                  // order pack in buffer
                  session.order_pack(Packet::new(pack_id, bytes.clone()));
                }

                // send Ack frame
                macros::write_frame!(
                  self.codec,
                  self.dst,
                  Frame::new(
                    *frame.uuid(),
                    Ack { pack_id }
                  ),
                  self.socket,
                  remote_addr
                );
              }
            );
          }

          // Ack frame
          Ack { pack_id } => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                // ack pack
                if let Some(packet) = session.ack_pack(pack_id) {
                  self.send_speed.add(packet.bytes().len());
                }
              }
            );
          }

          // Sync frame
          Sync { pack_id } => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                session.sync(*pack_id);
              }
            );
          }

          // Slow frame
          Slow { pack_id } => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                println!("[{}] reset send win", frame.uuid());
                session.reset_send_win();
                session.sync(*pack_id);
              }
            );
          }

          // Lost frame
          Lost { pack_id } => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                if let None = session.req_lost_pack(pack_id) {
                  println!(
                    "[{}] {:?} {:?} packet={} not found",
                    frame.uuid(),
                    session.state(),
                    session.last_acc_time().elapsed(),
                    pack_id);
                }
                session.sync(*pack_id);
              }
            );
          }

          // Shutdown frame
          Shutdown => {
            macros::get_session_or_wr_err!(
              self.session_map,
              frame.uuid(),
              self.codec,
              self.dst,
              self.socket,
              remote_addr,
              session => {
                // received Shutdown frame,
                // means no more data to come,
                // flush read buffer
                session.flush_rdbuf();
                if session.shutting_down() {
                  session.update_state(State::Close);
                }
              }
            );
          }

          // Close frame
          Close => {
            macros::get_session!(
              self.session_map,
              frame.uuid(),
              session => {
                // change session's state to Close
                session.update_state(State::Close);
              }
            );
          }

          // Error frame
          Error => {
            eprintln!("[{}] Error", frame.uuid());
            macros::get_session!(
              self.session_map,
              frame.uuid(),
              session => {
                session.drain(false);
              }
            );
          }
        };
        Ok(())
      }

      // not possible, but if happens, ignore
      Ok(None) => Ok(()),

      // decode error happens, return from loop
      Err(e) => Err(e),
    }
  }

  async fn recv_command(&mut self, res: Option<Command>) -> io::Result<()> {
    match res {
      // yes, got a command
      Some(cmd) => {
        match cmd {
          // udpack.connect()
          Command::Connect(s, dst_addr) => {
            let uuid = self.sf.next_id().unwrap();

            // push connect requet into queue
            self.connect_req_map.insert(uuid, (s, Instant::now()));

            // write Open(true) frame to open a transport
            macros::write_frame!(
              self.codec,
              self.dst,
              Frame::new(uuid, Open(true)),
              self.socket,
              dst_addr
            );
          }

          // udpack.shutdown()
          Command::ShutdownAll => {
            // send Shutdown frame for all sessions
            macros::shutdown_all!(self.session_map, self.connect_req_map, self.shutdown_all);
          }

          // transport.writable()
          Command::Writable(s, uuid) => {
            macros::get_session_or_err!(
              self.session_map,
              &uuid,
              s,
              session => {
                // send result over channel s
                macros::s_send_result_ignore_err!(
                  s,
                  Ok(!session.send_buf_overflow())
                );
              }
            );
          }

          // transport.write()
          Command::Write(s, uuid, bytes) => {
            macros::get_session_or_err!(
              self.session_map,
              &uuid,
              s,
              session => {
                // transport.shutdown() called,
                // and don't write any bytes.
                if session.flushing_wrbuf() {
                  // send err over channel s
                  macros::s_send_result_ignore_err!(
                    s,
                    macros::io_interrupted_err!("Transport may not write after shutdown")
                  );
                } else {
                  let bytes_written = bytes.len();
                  let step = Frame::MAX_DATA_LEN;
                  let mut start: usize = 0;
                  let mut end: usize = std::cmp::min(step, bytes_written);
                  while start < bytes_written {
                    // session assign pack id
                    let pack_id = session.next_pack_id();
                    // slice bytes [start..end)
                    let bytes = bytes.slice(start..end);
                    // encode data frame into bytes
                    macros::encode_frame!(
                      self.codec,
                      self.dst,
                      Frame::new(uuid, Data { pack_id, bytes })
                    );
                    // wrap bytes in the packet
                    let packet = Packet::new(
                      pack_id,
                      self.dst.copy_to_bytes(self.dst.len())
                    );
                    // push packet to the back of write buf
                    session.push_back_wrbuf(packet);
                    start = end;
                    end = std::cmp::min(end + step, bytes_written);
                  }
                  // send result over channel s
                  macros::s_send_result_ignore_err!(
                    s,
                    Ok(bytes_written)
                  );
                }
              }
            );
          }

          // transport.ping()
          Command::Ping(uuid) => {
            macros::get_session!(
              self.session_map,
              &uuid,
              session => {
                // stop write Ping after shutdown
                if let State::Ready = session.state() {
                  macros::write_frame!(
                    self.codec,
                    self.dst,
                    Frame::new(uuid, Ping),
                    self.socket,
                    session.remote_addr()
                  );
                }
              }
            );
          }

          // transport.remote_addr()
          Command::GetRemoteAddr(s, uuid) => {
            macros::get_session_or_err!(
              self.session_map,
              &uuid,
              s,
              session => {
                // send result over channel s
                macros::s_send_result_ignore_err!(
                  s,
                  Ok(session.remote_addr())
                );
              }
            );
          }

          // transport.shutdown()
          Command::Shutdown(uuid) => {
            macros::get_session_or!(
              self.session_map,
              &uuid,
              session => {
                session.shutdown();
              },
              _ => {
                eprintln!("[{}] session not found, shutdown failed", uuid);
              }
            );
          }

          // transport.close()
          Command::Close(uuid) => {
            macros::get_session_or!(
              self.session_map,
              &uuid,
              session => {
                session.drain(true);
              },
              _ => {
                eprintln!("[{}] session not found, close failed", uuid);
              }
            );
          }
        };
      }

      // udpack was dropped
      _ => {
        // shutdown all transports
        macros::shutdown_all!(self.session_map, self.connect_req_map, self.shutdown_all);
      }
    };
    Ok(())
  }

  fn check_expired_sessions(&mut self) {
    self
      .session_map
      .iter_mut()
      .filter(|(_, session)| session.expired())
      .for_each(|(_, session)| {
        session.drain(false);
      });
  }

  fn check_timeout_connects(&mut self) {
    // find timeout request
    let timeout_reqs: Vec<u64> = self
      .connect_req_map
      .iter()
      .filter(|(_, (_, req_time))| {
        req_time.elapsed() >= Duration::from_millis(CONNECT_TIMEOUT_MILLIS)
      })
      .map(|(uuid, _)| *uuid)
      .collect();

    // return timeout error
    for uuid in timeout_reqs {
      if let Some((s, _)) = self.connect_req_map.remove(&uuid) {
        macros::s_send_result_ignore_err!(s, macros::io_timeout_err!("connect timeout"));
        // drain the timeout session
        if let Some(session) = self.session_map.get_mut(&uuid) {
          session.drain(true);
        }
      }
    }
  }

  /// reset send window size of all sessions with retry ratio larger then Max
  fn retry_ratio_ctrl(&mut self) {
    self
      .session_map
      .iter_mut()
      .for_each(|(_, session)| session.retry_ratio_ctrl());
  }

  /// send packets in the write buffer
  async fn send_packets(&mut self) -> io::Result<()> {
    for (_, session) in &mut self.session_map {
      // get address of the remote peer
      let dst_addr = session.remote_addr();

      // send new arrival packets
      let mut send_pack_cnt = 0;
      'inner: while let Some(packet) = session.pop_front_wrbuf() {
        macros::send_packet!(
          self.socket,
          packet,
          dst_addr,
          send_pack_cnt,
          session.send_win(),
          // put packet into send_map for ack and retry
          done => session.send_pack(packet),
          // if already send 'send_win' packs, then
          // break 'inner
          'inner
        );
      }

      // increase send window size of the session
      session.increase_send_win();
    }
    Ok(())
  }

  async fn process_read_requests(&mut self) -> io::Result<()> {
    for (&uuid, session) in &mut self.session_map {
      let from = session.recv_pack_id();
      match session.recv_bytes() {
        Ok(bytes_read) => {
          self.recv_speed.add(bytes_read);
          // flushing rdbuf means all data received in read buffer or recv_map, and no more data will come,
          // don't send any other frames when flushing read buffer.
          if session.flushing_rdbuf() {
            // told transport's rx2 to be dropped
            if session.flushed_rdbuf() {
              if let Err(e) = session.recv_eof() {
                eprintln!("[{}] {}", uuid, e);
              }
            }
          } else {
            let sync = (session.recv_pack_id() - from) > 0;
            // recv buffer over flow,
            // tell the remote peer to send slowly.
            let slow = session.recv_buf_overflow();
            // found lost
            let lost = session.recv_waiting_lost();
            // send sync or slow or lost frame
            if sync || slow || lost {
              let pack_id = session.recv_pack_id();
              let kind = match lost {
                true => {
                  println!("[{}] {} lost", uuid, pack_id);
                  Lost { pack_id }
                }
                false => match slow {
                  true => Slow { pack_id },
                  false => Sync { pack_id },
                },
              };
              let remote_addr = session.remote_addr();
              macros::write_frame!(
                self.codec,
                self.dst,
                Frame::new(uuid, kind),
                self.socket,
                remote_addr
              );
            }
          }
        }
        Err(e) => {
          eprintln!("[{}] {}", uuid, e);
          session.recv_err();
        }
      };
    }
    Ok(())
  }

  async fn check_shutting_down(&mut self) -> io::Result<()> {
    for (&uuid, session) in &mut self.session_map {
      if session.flushed_wrbuf() {
        macros::write_shutdown_frame!(self.codec, self.dst, self.socket, session, uuid);
      }
    }

    // retain sessions that are not closed
    self.session_map.retain(|_uuid, session| !session.closed());
    Ok(())
  }

  /// check if all transports shutdown finished
  fn check_shutdown_all_finished(&self) -> Option<io::Result<()>> {
    if let Some(shutdown_time) = self.shutdown_all {
      // finished
      if self.session_map.len() == 0 {
        return Some(Ok(()));
      }

      // timeout
      if shutdown_time.elapsed() > Duration::from_millis(SHUTDOWN_TIMEOUT_MILLIS) {
        return Some(macros::io_timeout_err!("shutdown all timeout"));
      }
    }
    return None;
  }
}
