//!
//! # Examples
//!
//! ## server.rs
//! ```
//! use rust_udpack::Transport;
//! use rust_udpack::Udpack;
//! use std::io;
//! use tokio::signal;
//! use tokio::task::JoinHandle;
//!
//! #[tokio::main]
//! async fn main() -> io::Result<()> {
//!   let mut udpack: Udpack = Udpack::new("0.0.0.0:8080").await?;
//!
//!   loop {
//!     tokio::select! {
//!       res = udpack.accept() => {
//!         let _handle: JoinHandle<io::Result<()>> = tokio::spawn(async move {
//!           let mut transport: Transport = res.unwrap();
//!
//!           while let Some(bytes) = transport.read().await {
//!             println!("{:?}", bytes);
//!             transport.write(bytes).await?;
//!           }
//!           Ok(())
//!         });
//!       }
//!       _ = signal::ctrl_c() => {
//!         println!("ctrl-c received!");
//!         udpack.shutdown().await?;
//!         break;
//!       }
//!     }
//!   }
//!   Ok(())
//! }
//! ```
//!
//! ## client.rs
//! ```
//! use bytes::Bytes;
//! use rust_udpack::Transport;
//! use rust_udpack::Udpack;
//! use std::io;
//! use tokio::signal;
//! use tokio::time;
//! use tokio::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> io::Result<()> {
//!   let udpack: Udpack = Udpack::new("0.0.0.0:0").await?;
//!   let mut transport: Transport = udpack.connect("127.0.0.1:8080").await?;
//!   let mut interval = time::interval(Duration::from_secs(3));
//!
//!   loop {
//!     tokio::select! {
//!       res = transport.read() => {
//!         if let Some(bytes) = res {
//!           println!("{:?}", bytes);
//!         }
//!       }
//!       _ = interval.tick() => {
//!         transport.write(Bytes::copy_from_slice(&[1u8; 2048])).await?;
//!       }
//!       _ = signal::ctrl_c() => {
//!         println!("ctrl-c received!");
//!         udpack.shutdown().await?;
//!         return Ok(());
//!       }
//!     };
//!   }
//! }
//! ```

mod transport;
mod udpack;

#[cfg(test)]
mod tests;

pub use transport::Transport;
pub use udpack::Udpack;
