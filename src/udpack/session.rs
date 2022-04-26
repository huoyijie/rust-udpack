use crate::udpack::frame_task::CONNECT_TIMEOUT_MILLIS;
use crate::udpack::macros;
use crate::udpack::PackState;
use crate::udpack::Packet;
use crate::udpack::State;
use crate::udpack::State::Open;
use bytes::Bytes;
use std::collections::HashMap;
use std::collections::LinkedList;
use std::io;
use std::net::SocketAddr;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Duration;
use tokio::time::Instant;

const WAITING_MILLIS_BEFORE_LOST: u64 = 50;
const SESSION_EXPIRED_SECS: u64 = 60;
const SEND_BUFFER_MAX: usize = 10 * 1024 * 1024;
const RECV_BUFFER_MAX: usize = 10 * 1024 * 1024;
const SEND_WIN_MIN: u16 = 2;
const SEND_WIN_MAX: u16 = 128;
const SEND_WIN_STEP: u16 = 2;
const RETRY_TOTAL_MIN: u16 = 1024;
const RETRY_RATIO_MAX: f32 = 0.3;

/// Session for a connection
#[derive(Debug)]
pub struct Session {
  // session state
  state: State,

  // remote address
  remote_addr: SocketAddr,

  // decide if session is expired or not
  last_acc_time: Instant,

  // assign pack id
  serial_num: u32,

  // write buffer
  wr_buf_queue: LinkedList<Packet>,

  // send packets no more then every interval
  send_win: u16,

  // buffer packs for ack and retry
  send_map: HashMap<u32, Packet>,

  // Bytes Sender for transprot.read()
  tx2: UnboundedSender<Bytes>,

  // Bytes Receiver for transport.read()
  rx2: Option<UnboundedReceiver<Bytes>>,

  // buffer packs for ordering
  recv_map: HashMap<u32, Packet>,

  // next pack id that is receiving,
  // pack id smaller then recv_pack_id is received.
  recv_pack_id: u32,

  // sync recv_pack_id to peer
  sync_pack_id: u32,

  // (blocking on recv_pack_id since when, if lost)
  recv_waiting: (Instant, bool),

  // max pack id received ever
  max_recv_pack_id: u32,

  // transport.shutdown() called to flush write buffer
  flush_wrbuf: bool,

  // flush read buf before remove session
  flush_rdbuf: bool,

  // shutdown frame sent?
  shutdown_sent: bool,

  // close frame sent?
  close_sent: bool,

  // send error frame?
  send_err: bool,

  // recv eof?
  recv_eof: bool,
}

impl Session {
  pub fn new(state: State, remote_addr: SocketAddr) -> Self {
    let (tx2, rx2) = unbounded_channel();
    Self {
      state,
      remote_addr,
      last_acc_time: Instant::now(),
      serial_num: 0,
      wr_buf_queue: LinkedList::new(),
      send_win: SEND_WIN_MIN,
      send_map: HashMap::new(),
      tx2,
      rx2: Some(rx2),
      recv_map: HashMap::new(),
      recv_pack_id: 1,
      sync_pack_id: 1,
      recv_waiting: (Instant::now(), false),
      max_recv_pack_id: 0,
      flush_wrbuf: false,
      flush_rdbuf: false,
      shutdown_sent: false,
      close_sent: false,
      send_err: false,
      recv_eof: false,
    }
  }

  pub fn rx2(&mut self) -> Option<UnboundedReceiver<Bytes>> {
    self.rx2.take()
  }

  pub fn state(&self) -> State {
    self.state
  }

  pub fn update_state(&mut self, state: State) -> &mut Self {
    self.state = state;
    self
  }

  /// Whether the session should be deleted
  pub fn closed(&self) -> bool {
    self.flushed_wrbuf()
      && self.flushed_rdbuf()
      && (self.state == State::Close || self.state == State::Error)
  }

  pub fn shutting_down(&self) -> bool {
    self.state == State::Shutdown
  }

  pub fn shutdown(&mut self) -> &mut Self {
    let state = match self.state {
      State::Close => State::Close,
      _ => State::Shutdown,
    };
    self.update_state(state).flush_wrbuf();
    self
  }

  /// drain the session rudly
  pub fn drain(&mut self, send_err: bool) -> &mut Self {
    // flush read/write buffer before remove session
    self
      .update_state(State::Error)
      .discard_wrbuf()
      .discard_rdbuf()
      .send_err = send_err;
    self
  }

  pub fn send_err(&self) -> bool {
    self.send_err
  }

  pub fn err_sent(&mut self) {
    self.send_err = false;
  }

  pub fn remote_addr(&self) -> SocketAddr {
    self.remote_addr
  }

  pub fn update_remote_addr(&mut self, remote_addr: SocketAddr) -> &mut Self {
    self.remote_addr = remote_addr;
    self
  }

  pub fn last_acc_time(&self) -> Instant {
    self.last_acc_time
  }

  pub fn update_last_acc_time(&mut self) -> &mut Self {
    self.last_acc_time = Instant::now();
    self
  }

  /// Whether the session is expired
  pub fn expired(&self) -> bool {
    // session open timeout
    if let Open(open_time) = self.state {
      if open_time.elapsed() > Duration::from_millis(CONNECT_TIMEOUT_MILLIS) {
        return true;
      }
    }
    // session idle timeout
    return self.last_acc_time().elapsed() > Duration::from_secs(SESSION_EXPIRED_SECS);
  }

  pub fn next_pack_id(&mut self) -> u32 {
    self.serial_num += 1;
    self.serial_num
  }

  pub fn push_back_wrbuf(&mut self, packet: Packet) {
    self.wr_buf_queue.push_back(packet);
  }

  pub fn pop_front_wrbuf(&mut self) -> Option<Packet> {
    self.wr_buf_queue.pop_front()
  }

  pub fn shutdown_sent(&self) -> bool {
    self.shutdown_sent
  }

  pub fn send_shutdown(&mut self) {
    self.shutdown_sent = true;
  }

  pub fn close_sent(&self) -> bool {
    self.close_sent
  }

  pub fn send_close(&mut self) {
    self.close_sent = true;
  }

  pub fn flush_wrbuf(&mut self) {
    self.flush_wrbuf = true;
  }

  pub fn flushing_wrbuf(&self) -> bool {
    self.flush_wrbuf
  }

  pub fn flushed_wrbuf(&self) -> bool {
    self.flush_wrbuf && self.wr_buf_queue.is_empty() && self.send_map.is_empty()
  }

  pub fn flush_rdbuf(&mut self) {
    self.flush_rdbuf = true;
  }

  pub fn flushing_rdbuf(&self) -> bool {
    self.flush_rdbuf
  }

  pub fn flushed_rdbuf(&self) -> bool {
    self.flush_rdbuf && self.recv_map.is_empty()
  }

  pub fn discard_rdbuf(&mut self) -> &mut Self {
    self.flush_rdbuf();
    self.recv_map.clear();
    self
  }

  pub fn discard_wrbuf(&mut self) -> &mut Self {
    self.flush_wrbuf();
    self.wr_buf_queue.clear();
    self.send_map.clear();
    self
  }

  /// increase send window size step by step
  pub fn increase_send_win(&mut self) {
    // add SEND_WIN_STEP to send_win (up to SEND_WIN_MAX)
    if self.send_win + SEND_WIN_STEP <= SEND_WIN_MAX {
      self.send_win += SEND_WIN_STEP;
    }
  }

  /// retry ratio contrl
  pub fn retry_ratio_ctrl(&mut self) {
    let total = self.send_map.len() as u32;
    if total > RETRY_TOTAL_MIN as u32 {
      let retries = macros::pack_map_reduce!(self.send_map, packet => packet.retries());

      // retry ratio is larger then RETRY_RATIO_MAX,
      // reset send win
      if (retries as f32 / total as f32) > RETRY_RATIO_MAX {
        self.reset_send_win();
        println!("retry ratio exceed max {}, {}", retries, total);
      }
    }
  }

  /// send window size
  pub fn send_win(&self) -> u16 {
    self.send_win
  }

  /// Reset send window size
  pub fn reset_send_win(&mut self) {
    self.send_win = SEND_WIN_MIN;
  }

  /// Put the packet into the send buffer
  pub fn send_pack(&mut self, mut packet: Packet) {
    packet.set_state(PackState::Sent(Instant::now()));
    self.send_map.insert(packet.pack_id(), packet);
  }

  /// Ack received, and remove the packet from send buffer
  pub fn ack_pack(&mut self, pack_id: &u32) -> Option<Packet> {
    self.send_map.remove(pack_id)
  }

  /// Get all the timeout packets
  pub fn timeout_packs(&mut self) -> Vec<&mut Packet> {
    let mut packs = self
      .send_map
      .iter_mut()
      .filter(|(_, packet)| packet.timeout())
      .map(|(_, packet)| packet)
      .collect::<Vec<&mut Packet>>();
    packs.sort();
    return packs;
  }

  /// Get all the lost packets
  pub fn lost_packs(&mut self) -> Vec<&mut Packet> {
    self
      .send_map
      .iter_mut()
      .filter(|(_, packet)| packet.lost())
      .map(|(_, packet)| packet)
      .collect()
  }

  /// Put the packet in the recv buffer for ordring
  pub fn order_pack(&mut self, mut packet: Packet) {
    packet.set_state(PackState::Ack);
    if packet.pack_id() > self.max_recv_pack_id {
      self.max_recv_pack_id = packet.pack_id();
    }
    self.recv_map.insert(packet.pack_id(), packet);
  }

  /// receiving bytes in recv buffer
  pub fn recv_bytes(&mut self) -> io::Result<usize> {
    let mut bytes_read = 0;
    while let Some(packet) = self.recv_map.remove(&self.recv_pack_id) {
      self.recv_pack_id += 1;
      self.recv_waiting = (Instant::now(), false);

      let bytes = packet.bytes();
      bytes_read += bytes.len();

      macros::s_send_result!(
        self.tx2,
        bytes,
        _ => return macros::io_interrupted_err!("recv bytes failed; err = the rx2 dropped")
      );
    }
    return Ok(bytes_read);
  }

  /// There are no more data in recv buffer,
  /// send eof signal to the data consumer.
  pub fn recv_eof(&mut self) -> io::Result<()> {
    if !self.recv_eof {
      self.recv_eof = true;
      macros::s_send_result!(
        self.tx2,
        Bytes::copy_from_slice(&[]),
        _ => return macros::io_interrupted_err!("recv eof failed; err = the rx2 dropped")
      );
    }
    return Ok(());
  }

  /// No data consumers anymore,
  /// the bytes in the recv buffer are no more needed.
  pub fn recv_err(&mut self) {
    self.recv_eof = true;
    self.discard_rdbuf();
  }

  /// send buffer over flow
  pub fn send_buf_overflow(&self) -> bool {
    let queue_bytes = macros::pack_list_map_reduce!(self.wr_buf_queue);
    let buf_bytes = macros::pack_map_reduce!(self.send_map, packet => packet.bytes().len());
    return queue_bytes + buf_bytes >= SEND_BUFFER_MAX;
  }

  /// recv buffer over flow
  pub fn recv_buf_overflow(&self) -> bool {
    macros::pack_map_reduce!(self.recv_map, packet => packet.bytes().len()) >= RECV_BUFFER_MAX
  }

  /// Whether the packet was received
  pub fn pack_received(&self, pack_id: u32) -> bool {
    pack_id < self.recv_pack_id || self.recv_map.contains_key(&pack_id)
  }

  /// Get the pack_id the session is receiving
  pub fn recv_pack_id(&self) -> u32 {
    self.recv_pack_id
  }

  /// Whether the packet the session is waiting to receive is lost
  pub fn recv_waiting_lost(&mut self) -> bool {
    let lost = self.max_recv_pack_id() > self.recv_pack_id()
      && !self.recv_waiting.1 // not found lost
      && self.recv_waiting.0.elapsed() > Duration::from_millis(WAITING_MILLIS_BEFORE_LOST); // waiting so long
    if lost {
      self.recv_waiting.1 = true;
    }
    return lost;
  }

  /// The largest pack_id received so far
  pub fn max_recv_pack_id(&self) -> u32 {
    self.max_recv_pack_id
  }

  /// Ack packets (which pack_id less then sync_pack_id) are all received by the other peer
  pub fn sync(&mut self, sync_pack_id: u32) {
    for pack_id in self.sync_pack_id..sync_pack_id {
      self.ack_pack(&pack_id);
    }
    self.sync_pack_id = sync_pack_id;
  }

  /// Get the lost packet, and set it's state to Lost
  pub fn req_lost_pack(&mut self, lost_pack_id: &u32) -> Option<&mut Packet> {
    if let Some(packet) = self.send_map.get_mut(lost_pack_id) {
      Some(packet.set_state(PackState::Lost))
    } else {
      None
    }
  }
}
