use bytes::Bytes;
use rust_udpack::{Transport, Udpack};
use std::io;
use tokio::{
  signal,
  time::{interval, Duration},
};

#[tokio::main]
async fn main() -> io::Result<()> {
  let udpack: Udpack = Udpack::new("0.0.0.0:0").await?;
  let mut transport: Transport = udpack.connect("127.0.0.1:8080").await?;
  let mut interval = interval(Duration::from_secs(3));

  loop {
    tokio::select! {
      res = transport.read() => {
        if let Some(bytes) = res {
          println!("{:?}", bytes);
        }
      }
      _ = interval.tick() => {
        transport.write(Bytes::copy_from_slice(&[1u8; 2048])).await?;
      }
      _ = signal::ctrl_c() => {
        println!("ctrl-c received!");
        udpack.shutdown().await?;
        return Ok(());
      }
    };
  }
}
