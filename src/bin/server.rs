use rust_udpack::{Transport, Udpack};
use std::io;
use tokio::{signal, task::JoinHandle};

#[tokio::main]
async fn main() -> io::Result<()> {
  let mut udpack: Udpack = Udpack::new("0.0.0.0:8080").await?;

  loop {
    tokio::select! {
      res = udpack.accept() => {
        let _handle: JoinHandle<io::Result<()>> = tokio::spawn(async move {
          let mut transport: Transport = res.unwrap();

          while let Some(bytes) = transport.read().await {
            println!("{:?}", bytes);
            transport.write(bytes).await?;
          }
          Ok(())
        });
      }
      _ = signal::ctrl_c() => {
        println!("ctrl-c received!");
        udpack.shutdown().await?;
        break;
      }
    }
  }
  Ok(())
}
