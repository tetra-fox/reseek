//! A scripted peer: the far end of a peer, transfer or distributed connection, for what two
//! clients on soulfind cannot show. It reads and writes whole frames with the protocol codecs.

use std::net::{Ipv4Addr, SocketAddr};

use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::{DistributedMessage, InitMessage, Limits, PeerMessage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::PATIENCE;

/// A socket a scripted peer listens on
pub struct FakePeer {
  listener: TcpListener,
}

impl FakePeer {
  pub async fn start() -> Self {
    Self {
      listener: TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap(),
    }
  }

  pub fn address(&self) -> SocketAddr {
    self.listener.local_addr().unwrap()
  }

  pub fn port(&self) -> u32 {
    u32::from(self.address().port())
  }

  /// the next connection the client makes to us
  pub async fn accept(&self) -> PeerSide {
    let (stream, _) = tokio::time::timeout(PATIENCE, self.listener.accept())
      .await
      .expect("the client connects")
      .unwrap();
    PeerSide::new(stream)
  }
}

/// The scripted peer's end of one connection
pub struct PeerSide {
  pub stream: TcpStream,
  buffer: Vec<u8>,
}

impl PeerSide {
  pub fn new(stream: TcpStream) -> Self {
    Self {
      stream,
      buffer: Vec::new(),
    }
  }

  /// connects to a client's listener
  pub async fn connect(address: SocketAddr) -> Self {
    Self::new(TcpStream::connect(address).await.unwrap())
  }

  /// connects to a client's listener from the loopback address `from`, such as 127.0.0.2
  pub async fn connect_from(from: Ipv4Addr, address: SocketAddr) -> Self {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind(SocketAddr::from((from, 0))).unwrap();
    Self::new(socket.connect(address).await.unwrap())
  }

  /// reads what has arrived into the buffer; false once the client closed the connection
  async fn fill(&mut self) -> bool {
    let mut chunk = vec![0; 64 << 10];
    let read = tokio::time::timeout(PATIENCE, self.stream.read(&mut chunk))
      .await
      .expect("the client sends something or closes")
      .unwrap_or(0);
    self.buffer.extend_from_slice(&chunk[..read]);
    read > 0
  }

  /// the next whole frame of `kind`
  pub async fn frame(&mut self, kind: FrameKind) -> Vec<u8> {
    self
      .frame_or_close(kind)
      .await
      .expect("the client closed the connection")
  }

  /// the next whole frame of `kind`, or `None` when the client closes the connection first
  pub async fn frame_or_close(&mut self, kind: FrameKind) -> Option<Vec<u8>> {
    loop {
      if let Some((_, consumed)) = frame::split(kind, &self.buffer, &Limits::DEFAULT).unwrap() {
        return Some(self.buffer.drain(..consumed).collect());
      }
      if !self.fill().await {
        return None;
      }
    }
  }

  /// exactly `len` raw bytes, for what follows the first message of a transfer connection
  pub async fn raw(&mut self, len: usize) -> Vec<u8> {
    while self.buffer.len() < len {
      assert!(self.fill().await, "the client closed the connection");
    }
    self.buffer.drain(..len).collect()
  }

  pub async fn read_init(&mut self) -> InitMessage {
    InitMessage::decode(&self.frame(FrameKind::Init).await).unwrap()
  }

  pub async fn read_peer(&mut self) -> PeerMessage {
    PeerMessage::decode(&self.frame(FrameKind::Peer).await).unwrap()
  }

  pub async fn read_distributed(&mut self) -> DistributedMessage {
    DistributedMessage::decode(&self.frame(FrameKind::Distributed).await).unwrap()
  }

  pub async fn send(&mut self, bytes: &[u8]) {
    self.stream.write_all(bytes).await.unwrap();
  }

  /// waits for the client to close the connection, failing if it sends a frame first
  pub async fn closed(&mut self) {
    assert!(self.buffer.is_empty(), "unread bytes before the close");
    let mut chunk = [0; 1024];
    let read = tokio::time::timeout(PATIENCE, self.stream.read(&mut chunk))
      .await
      .expect("the client closes the connection")
      .unwrap_or(0);
    assert_eq!(read, 0, "the client sent more before closing");
  }

  /// whether the client sends nothing for `wait`
  pub async fn quiet_for(&mut self, wait: std::time::Duration) -> bool {
    if !self.buffer.is_empty() {
      return false;
    }
    let mut chunk = [0; 1024];
    match tokio::time::timeout(wait, self.stream.read(&mut chunk)).await {
      Err(_) => true,
      Ok(Ok(read)) => {
        self.buffer.extend_from_slice(&chunk[..read]);
        false
      }
      Ok(Err(_)) => false,
    }
  }
}
