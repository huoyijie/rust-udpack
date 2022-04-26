use tokio::time::Instant;

#[derive(Debug, Copy, Clone)]
pub enum PackState {
  Init,
  Sent(Instant),
  Lost,
  Retry,
  Ack,
}
