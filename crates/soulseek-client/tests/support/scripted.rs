//! A client on the scripted server whose transfers go to a scripted peer, for the transfer paths
//! two real clients never take: refusals with every reason, failures, peers that lie or vanish
//! at each stage, and canceling at each stage.

use std::path::PathBuf;

use soulseek_client::{
  Client, ClientError, ClientOptions, DownloadOutput, DownloadRequest, Transfer,
};
use soulseek_proto::init::{FileTransferInit, PeerInit};
use soulseek_proto::peer::{QueueDownload, TransferRequest, TransferResponse};
use soulseek_proto::server::{GetPeerAddressRequest, GetPeerAddressResponse};
use soulseek_proto::{
  InitMessage, PeerConnectionType, PeerMessage, ServerRequest, ServerResponse, TransferDirection,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::peer::{FakePeer, PeerSide};
use super::transfers::{content, scratch};
use super::{PATIENCE, Session, USERNAME, listening_options};

/// the scripted peer's name
pub const BOB: &str = "bob";

/// A listening client on the scripted server and bob, a scripted peer the server gives out the
/// address of
pub struct Scripted {
  pub session: Session,
  pub bob: FakePeer,
}

impl Scripted {
  pub async fn start() -> Self {
    Self::with_options(listening_options()).await
  }

  pub async fn with_options(options: ClientOptions) -> Self {
    Self {
      session: Session::with_options(options).await,
      bob: FakePeer::start().await,
    }
  }

  pub fn client(&self) -> &Client {
    &self.session.client
  }

  /// answers the client's next lookup of bob's address with the scripted peer's
  pub async fn give_address(&mut self) {
    self
      .session
      .server
      .request_matching(|request| match request {
        ServerRequest::GetPeerAddress(GetPeerAddressRequest { username }) if username == BOB => {
          Some(())
        }
        _ => None,
      })
      .await;
    self
      .session
      .server
      .send(&ServerResponse::GetPeerAddress(GetPeerAddressResponse {
        username: BOB.into(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: self.bob.port(),
        obfuscation: None,
      }))
      .await;
  }

  /// gives the client bob's address and returns the message connection it then opens to bob
  pub async fn connected(&mut self) -> PeerSide {
    self.give_address().await;
    self.accept_message().await
  }

  /// the next message connection the client opens to bob, past its introduction
  pub async fn accept_message(&self) -> PeerSide {
    let mut side = self.bob.accept().await;
    let InitMessage::PeerInit(PeerInit {
      username,
      connection_type,
      ..
    }) = side.read_init().await
    else {
      panic!("the client introduces itself");
    };
    assert_eq!(
      (username.as_str(), connection_type),
      (USERNAME, PeerConnectionType::Peer)
    );
    side
  }

  /// the next transfer connection the client opens to bob, and the token it names
  pub async fn accept_transfer(&self) -> (PeerSide, u32) {
    let mut side = self.bob.accept().await;
    let InitMessage::PeerInit(PeerInit {
      connection_type, ..
    }) = side.read_init().await
    else {
      panic!("the client introduces itself");
    };
    assert_eq!(connection_type, PeerConnectionType::Transfer);
    let token = FileTransferInit::decode(&side.raw(4).await).unwrap().token;
    (side, token)
  }

  /// bob opens a transfer connection to the client for the transfer the client named `token`
  pub async fn connect_for_transfer(&self, token: u32) -> PeerSide {
    let mut side = PeerSide::connect(self.session.listen_address()).await;
    let mut init = InitMessage::PeerInit(PeerInit {
      username: BOB.into(),
      connection_type: PeerConnectionType::Transfer,
      token: 0,
    })
    .encode();
    init.extend(FileTransferInit { token }.encode());
    side.send(&init).await;
    side
  }
}

/// the size of the files bob sends
pub const SIZE: u64 = 50_000;

/// A download from bob running on a task, with what cancels it
pub struct Downloading {
  pub task: JoinHandle<Result<Transfer, ClientError>>,
  pub cancel: CancellationToken,
  pub destination: PathBuf,
}

impl Downloading {
  pub async fn result(self) -> Result<Transfer, ClientError> {
    tokio::time::timeout(PATIENCE, self.task)
      .await
      .expect("the download ends")
      .unwrap()
  }
}

impl Scripted {
  /// starts downloading `filename` from bob, with the request shaped by `shape`
  pub fn download(&self, filename: &str, shape: impl FnOnce(&mut DownloadRequest)) -> Downloading {
    let destination = scratch().join("download");
    let mut request = DownloadRequest::new(
      BOB,
      filename,
      SIZE,
      DownloadOutput::File(destination.clone()),
    );
    shape(&mut request);
    let cancel = CancellationToken::new();
    let client = self.client().clone();
    let task = tokio::spawn({
      let cancel = cancel.clone();
      async move { client.download(request, &cancel).await }
    });
    Downloading {
      task,
      cancel,
      destination,
    }
  }

  /// a download from bob queued at bob, with bob's side of the message connection
  pub async fn queued_download(&mut self, filename: &str) -> (Downloading, PeerSide) {
    let running = self.download(filename, |_| {});
    let mut bob = self.connected().await;
    queued(&mut bob, filename).await;
    (running, bob)
  }
}

/// bob's side reads the client's `QueueDownload` for `filename`
pub async fn queued(bob: &mut PeerSide, filename: &str) {
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::QueueDownload(QueueDownload {
      filename: filename.into(),
    })
  );
}

/// bob offers to start sending `filename` with token `token` and reads the client's acceptance
pub async fn offer(bob: &mut PeerSide, filename: &str, token: u32, size: Option<u64>) {
  bob
    .send(
      &PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload,
        token,
        filename: filename.into(),
        file_size: size,
      })
      .encode(),
    )
    .await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Allowed {
      token,
      file_size: None,
    })
  );
}

/// bob reads the start offset the client asks for, checks it is `start`, and sends `len` bytes
/// of the file from there on the transfer connection
pub async fn send_file(transfer: &mut PeerSide, start: u64, len: usize) {
  let offset = u64::from_le_bytes(transfer.raw(8).await.try_into().unwrap());
  assert_eq!(offset, start);
  let file = content(SIZE as usize);
  transfer
    .send(&file[start as usize..start as usize + len])
    .await;
}
