use tokio::time::Instant;

/// Calculate the rate of sending and receiving data.
pub struct Speed {
  bytes: usize,
  from: Instant,
}

#[allow(dead_code)]
impl Speed {
  pub fn new() -> Self {
    Self {
      bytes: 0,
      from: Instant::now(),
    }
  }

  /// add bytes
  pub fn add(&mut self, bytes: usize) {
    self.bytes += bytes;
  }

  /// speed in bytes per second.
  pub fn in_bytes(&self) -> f32 {
    self.bytes as f32 / self.from.elapsed().as_secs_f32()
  }

  /// speed in kilo bytes per second.
  pub fn in_kilo_bytes(&self) -> String {
    format!("{:.1} KB/s", self.in_bytes() / 1024.0)
  }

  /// reset bytes and time
  pub fn reset(&mut self) {
    self.bytes = 0;
    self.from = Instant::now();
  }
}
