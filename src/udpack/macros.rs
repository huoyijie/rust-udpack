macro_rules! io_interrupted_err {
  ($err_str: expr) => {
    Err(std::io::Error::new(
      std::io::ErrorKind::Interrupted,
      $err_str,
    ))
  };
}

macro_rules! io_eof_err {
  ($err_str: expr) => {
    Err(std::io::Error::new(
      std::io::ErrorKind::UnexpectedEof,
      $err_str,
    ))
  };
}

macro_rules! io_invalid_data_err {
  ($err_str: expr) => {
    Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      $err_str,
    ))
  };
}

macro_rules! io_timeout_err {
  ($err_str: expr) => {
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, $err_str))
  };
}

macro_rules! ignore_err {
  ($res: expr) => {
    if let Err(_) = $res {}
  };
}

macro_rules! tx_send {
  ($tx: expr, $cmd: expr, $err_str: expr) => {
    if let Err(e) = $tx.send($cmd) {
      return crate::udpack::macros::io_eof_err!(format!("{}; err = {}", $err_str, e));
    }
  };
}

macro_rules! r {
  ($r: ident, $err_str: expr) => {
    match $r.await {
      Err(e) => crate::udpack::macros::io_eof_err!(format!("{}; err = {}", $err_str, e)),
      Ok(res) => res,
    }
  };
}

macro_rules! get_session_or {
  ($session_map: expr, $uuid: expr, $session: ident => $if_block: block, _ => $else_block: block) => {
    if let Some($session) = $session_map.get_mut($uuid)
      $if_block
    else
      $else_block
  };
}

macro_rules! get_session {
  ($session_map: expr, $uuid: expr, $session: ident => $if_block: block) => {
    crate::udpack::macros::get_session_or!(
      $session_map,
      $uuid,
      $session => $if_block,
      _ => {}
    );
  };
}

macro_rules! get_session_or_wr_err {
  ($session_map: expr, $uuid: expr, $codec: expr, $dst: expr, $socket: expr, $remote_addr: ident, $session: ident => $if_block: block) => {
    use crate::udpack::frame::Frame;
    use crate::udpack::frame_kind::FrameKind;
    crate::udpack::macros::get_session_or!(
      $session_map,
      $uuid,
      $session => $if_block,
      _ => {
        let frame = Frame::new(*$uuid, FrameKind::Error);
        crate::udpack::macros::write_frame!($codec, $dst, frame, $socket, $remote_addr);
      }
    );
  };
}

macro_rules! get_session_or_err {
  ($session_map: expr, $uuid: expr, $s: expr, $session: ident => $if_block: block) => {
    crate::udpack::macros::get_session_or!(
      $session_map,
      $uuid,
      $session => $if_block,
      _ => {
        crate::udpack::macros::s_send_result_ignore_err!(
          $s,
          crate::udpack::macros::io_interrupted_err!(
            format!("[{}] session not found", $uuid)
          )
        );
      }
    );
  };
}

macro_rules! write_shutdown_frame {
  ($codec: expr, $dst: expr, $socket: expr, $session: ident, $uuid: expr) => {
    use crate::udpack::frame::Frame;
    use crate::udpack::frame_kind::FrameKind;
    use crate::udpack::state::State;
    let remote_addr = $session.remote_addr();
    match $session.state() {
      State::Shutdown => {
        if $session.flushed_wrbuf() && !$session.shutdown_sent() {
          // write shutdown frame
          let frame = Frame::new($uuid, FrameKind::Shutdown);
          crate::udpack::macros::write_frame!($codec, $dst, frame, $socket, remote_addr);
          $session.send_shutdown();
        }
      }
      State::Close => {
        if $session.flushed_rdbuf() && !$session.close_sent() {
          // write close frame
          let frame = Frame::new($uuid, FrameKind::Close);
          crate::udpack::macros::write_frame!($codec, $dst, frame, $socket, remote_addr);
          $session.send_close();
        }
      }
      State::Error => {
        if $session.send_err() && $session.flushed_wrbuf() && $session.flushed_rdbuf() {
          // write error frame
          let frame = Frame::new($uuid, FrameKind::Error);
          crate::udpack::macros::write_frame!($codec, $dst, frame, $socket, remote_addr);
          $session.err_sent();
        }
      }
      _ => {}
    };
  };
}

macro_rules! shutdown_all {
  ($session_map: expr, $connect_req_map: expr, $shutdown_all: expr) => {
    // clear all connecting request
    for (_, (s, _)) in $connect_req_map.drain() {
      crate::udpack::macros::s_send_result_ignore_err!(
        s,
        crate::udpack::macros::io_interrupted_err!("connect interrupted")
      );
    }

    // flush write buffer before shutdown
    for (_, session) in &mut $session_map {
      session.drain(true);
    }

    // record shutdown time
    $shutdown_all = Some(tokio::time::Instant::now());
  };
}

macro_rules! encode_frame {
  ($codec: expr, $dst: expr, $frame: expr) => {
    use tokio_util::codec::Encoder;
    if let Err(e) = $codec.encode($frame, &mut $dst) {
      return crate::udpack::macros::io_invalid_data_err!(format!(
        "encode frame failed; err = {}",
        e
      ));
    }
  };
}

macro_rules! socket_send_to {
  ($socket: expr, $bytes: expr, $remote_addr: expr) => {
    // send bytes to the remote addr.
    $socket.send_to(&$bytes, $remote_addr).await?;
  };
}

macro_rules! write_frame {
  ($codec: expr, $dst: expr, $frame: expr, $socket: expr, $remote_addr: expr) => {
    // encode frame to bytes, and put into the dst buffer.
    crate::udpack::macros::encode_frame!($codec, $dst, $frame);

    use bytes::Buf;
    // consume all bytes in the dst buffer.
    let bytes = $dst.copy_to_bytes($dst.len());

    // // send bytes to the remote addr.
    crate::udpack::macros::socket_send_to!($socket, &bytes, $remote_addr);
  };
}

macro_rules! retry_lost_packets {
  ($socket: expr, $session_map: expr) => {
    for (_, session) in &mut $session_map {
      let dst_addr = session.remote_addr();
      for packet in session.lost_packs() {
        crate::udpack::macros::socket_send_to!($socket, &packet.bytes(), dst_addr);
        packet.set_state(PackState::Sent(Instant::now()));
      }
    }
  };
}

macro_rules! retry_timeout_packets {
  ($socket: expr, $session_map: expr, $max: expr) => {{
    let mut send_pack_cnt = 0;
    'outer: for (_, session) in &mut $session_map {
      let dst_addr = session.remote_addr();
      for packet in session.timeout_packs() {
        packet.set_state(PackState::Retry);
        crate::udpack::macros::socket_send_to!($socket, &packet.bytes(), dst_addr);
        packet.set_state(PackState::Sent(Instant::now()));
        send_pack_cnt += 1;
        if send_pack_cnt >= $max {
          break 'outer;
        }
      }
    }
  }};
}

macro_rules! send_packet {
  ($socket: expr, $packet: ident, $dst_addr: ident, $send_pack_cnt: ident, $send_win: expr, done => $done: stmt, $label: tt) => {
    // send packet to dst_addr over socket
    crate::udpack::macros::socket_send_to!($socket, &$packet.bytes(), $dst_addr);
    // send_pack_cnt++
    $send_pack_cnt += 1;
    $done
    // send no more then send_win packs every interval
    if $send_pack_cnt >= $send_win {
      break $label;
    }
  };
}

macro_rules! s_send_transport {
  ($s: ident, $transport: ident) => {
    crate::udpack::macros::s_send_result!(
      $s,
      Ok($transport),
      // r in udpack.connect() dropped
      // no where care abount the transport, so should shutdown it.
      Ok(transport) => transport.shutdown()?
    );
  };
}

macro_rules! s_send_result {
  ($s: expr, $res: expr, $send_result: pat => $on_error: stmt) => {
    if let Err($send_result) = $s.send($res) {
      $on_error
    }
  };
}

macro_rules! s_send_result_ignore_err {
  ($s: expr, $res: expr) => {
    if let Err(_) = $s.send($res) {}
  };
}

macro_rules! pack_map_reduce {
  ($map: expr, $packet: ident => $mapping: expr) => {
    $map
      .iter()
      .map(|(_, $packet)| $mapping)
      .reduce(|accum, item| accum + item)
      .unwrap_or(0)
  };
}

macro_rules! pack_list_map_reduce {
  ($map: expr) => {
    $map
      .iter()
      .map(|packet| packet.bytes().len())
      .reduce(|accum, item| accum + item)
      .unwrap_or(0)
  };
}

pub(crate) use {
  encode_frame, get_session, get_session_or, get_session_or_err, get_session_or_wr_err, ignore_err,
  io_eof_err, io_interrupted_err, io_invalid_data_err, io_timeout_err, pack_list_map_reduce,
  pack_map_reduce, r, retry_lost_packets, retry_timeout_packets, s_send_result,
  s_send_result_ignore_err, s_send_transport, send_packet, shutdown_all, socket_send_to, tx_send,
  write_frame, write_shutdown_frame,
};
