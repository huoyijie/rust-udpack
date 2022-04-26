use tokio::time::Instant;

/// State of the Transport instance.
/// ```
/// +--------------------------------+
/// |create session                  |
/// +--------------------------------+
/// |                >>      Open    |
/// |Open            <<ack           |
/// |Ready           >>      Ready   |
/// +--------------------------------+
///
/// +--------------------------------+
/// | close session                  |
/// +--------------------------------+
/// |Shutdown        >>      Shutdown|
/// |                <<ack           |
/// |Close           >>      Close   |
/// |[Drop]          <<ack   [Drop]  |
/// +--------------------------------+
/// ```
/// Instances in the Error state are immediately deleted.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum State {
  Open(Instant),
  Ready,
  Shutdown,
  Close,
  Error,
}
