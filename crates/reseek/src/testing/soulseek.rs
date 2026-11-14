//! A scripted Soulseek server on loopback, for what the daemon does with a session: it accepts
//! every login, answers our own statistics and privileges, answers the other requests as the
//! test's script says, hands on every request it reads, and sends or closes when told.
//! [`next_frame`] reads what the daemon writes to a scripted peer.

use std::net::Ipv4Addr;
use std::time::Duration;

use soulseek_client::ClientState;
use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::server::{
  CheckPrivilegesResponse, LoginResponse, UserData, UserStatistics, UserStats,
};
use soulseek_proto::{Limits, Secret, ServerRequest, ServerResponse, UserPresence};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::TestApp;

/// The name [`Server::login`] logs in as.
pub const US: &str = "me";

/// How long [`Server::received`] waits, longer than the client waits for an answer.
const RECEIVED_WITHIN: Duration = Duration::from_secs(8);

/// A daemon to log in to a [`Server`], with the `config.toml` `config`: it listens for peers on
/// a loopback port of its own, since tests run at once, and asks no router to forward it.
pub async fn app(config: &str) -> TestApp {
  let port = super::ports::free_port().to_string();
  let env = [
    ("RESEEK_SLSK_LISTEN_IP_ADDRESS", "127.0.0.1"),
    ("RESEEK_SLSK_LISTEN_PORT", port.as_str()),
    ("RESEEK_SLSK_PORT_MAPPING", "false"),
  ];
  super::app(config, &env).await
}

enum Control {
  Send(Vec<ServerResponse>),
  Close,
}

/// The server: one connection at a time, the next accepted once the one before closed.
pub struct Server {
  /// the loopback port it listens on
  pub port: u16,
  requests: mpsc::UnboundedReceiver<ServerRequest>,
  control: mpsc::UnboundedSender<Control>,
}

impl Server {
  /// A server answering each request with what `script` gives for it, besides the login and
  /// the statistics and privileges requests, which it answers itself.
  pub async fn start(
    script: impl FnMut(&ServerRequest) -> Vec<ServerResponse> + Send + 'static,
  ) -> Self {
    Self::serving(script, true).await
  }

  /// [`Server::start`] whose `script` answers the statistics requests too, so a test can hold
  /// some back.
  pub async fn start_scripting_statistics(
    script: impl FnMut(&ServerRequest) -> Vec<ServerResponse> + Send + 'static,
  ) -> Self {
    Self::serving(script, false).await
  }

  async fn serving(
    mut script: impl FnMut(&ServerRequest) -> Vec<ServerResponse> + Send + 'static,
    statistics: bool,
  ) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sent, requests) = mpsc::unbounded_channel();
    let (control, mut controls) = mpsc::unbounded_channel();
    tokio::spawn(async move {
      while let Ok((stream, _)) = listener.accept().await {
        if !serve(stream, &mut script, statistics, &sent, &mut controls).await {
          return;
        }
      }
    });
    Self {
      port,
      requests,
      control,
    }
  }

  /// A server that answers nothing but what every server here does.
  pub async fn silent() -> Self {
    Self::start(|_| Vec::new()).await
  }

  /// Logs the client of `app` in as [`US`].
  pub async fn login(&self, app: &TestApp) {
    app
      .state
      .network
      .client()
      .connect(
        "127.0.0.1",
        self.port,
        US,
        "secret",
        &CancellationToken::new(),
      )
      .await
      .unwrap();
  }

  /// Closes the connection and waits until the client of `app` is disconnected.
  pub async fn drop_session(&self, app: &TestApp) {
    self.control.send(Control::Close).unwrap();
    let client = app.state.network.client();
    tokio::time::timeout(Duration::from_secs(5), async {
      while client.state() != ClientState::Disconnected {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .expect("the client never noticed the closed connection");
  }

  /// Sends `messages` to the client in one write.
  pub fn send(&self, messages: Vec<ServerResponse>) {
    self.control.send(Control::Send(messages)).unwrap();
  }

  /// The next request `pick` takes, skipping the others; panics when none comes in time.
  pub async fn received<T>(&mut self, pick: impl Fn(&ServerRequest) -> Option<T>) -> T {
    tokio::time::timeout(RECEIVED_WITHIN, async {
      loop {
        let request = self.requests.recv().await.expect("the server stopped");
        if let Some(picked) = pick(&request) {
          return picked;
        }
      }
    })
    .await
    .expect("the request never came")
  }

  /// Whether a request `is` comes within `wait`, skipping the others.
  pub async fn receives(&mut self, wait: Duration, is: impl Fn(&ServerRequest) -> bool) -> bool {
    tokio::time::timeout(wait, async {
      loop {
        match self.requests.recv().await {
          Some(request) if is(&request) => return true,
          Some(_) => {}
          None => return false,
        }
      }
    })
    .await
    .unwrap_or(false)
  }
}

/// Serves one connection until it closes, answering the statistics requests itself when
/// `statistics`; false when the test is done with the server.
async fn serve(
  mut stream: TcpStream,
  script: &mut (impl FnMut(&ServerRequest) -> Vec<ServerResponse> + Send),
  statistics: bool,
  sent: &mpsc::UnboundedSender<ServerRequest>,
  controls: &mut mpsc::UnboundedReceiver<Control>,
) -> bool {
  let mut buffer = Vec::new();
  loop {
    while let Some((_, consumed)) =
      frame::split(FrameKind::Server, &buffer, &Limits::DEFAULT).unwrap()
    {
      let frame: Vec<u8> = buffer.drain(..consumed).collect();
      let request = ServerRequest::decode(&frame).unwrap();
      let mut answers = always(&request, statistics);
      answers.extend(script(&request));
      if !answers.is_empty() && stream.write_all(&encoded(&answers)).await.is_err() {
        return true;
      }
      if sent.send(request).is_err() {
        return false;
      }
    }
    let mut chunk = [0; 4096];
    tokio::select! {
      read = stream.read(&mut chunk) => match read {
        Ok(read) if read > 0 => buffer.extend_from_slice(&chunk[..read]),
        _ => return true,
      },
      control = controls.recv() => match control {
        Some(Control::Send(messages)) => {
          if stream.write_all(&encoded(&messages)).await.is_err() {
            return true;
          }
        }
        Some(Control::Close) => return true,
        None => return false,
      },
    }
  }
}

fn encoded(messages: &[ServerResponse]) -> Vec<u8> {
  messages.iter().flat_map(ServerResponse::encode).collect()
}

/// What every server here answers: the login, our privileges and, when `statistics`, any user's
/// statistics, which the daemon asks for after each login.
fn always(request: &ServerRequest, statistics: bool) -> Vec<ServerResponse> {
  match request {
    ServerRequest::Login(_) => vec![ServerResponse::Login(LoginResponse::Success {
      greeting: "welcome".into(),
      ip: Ipv4Addr::new(203, 0, 113, 7),
      hash: Secret::new("0123456789abcdef0123456789abcdef"),
      is_supporter: false,
    })],
    ServerRequest::GetUserStats(asked) if statistics => {
      vec![ServerResponse::GetUserStats(UserStatistics {
        username: asked.username.clone(),
        stats: UserStats::default(),
      })]
    }
    ServerRequest::CheckPrivileges => {
      vec![ServerResponse::CheckPrivileges(CheckPrivilegesResponse {
        seconds_remaining: 0,
      })]
    }
    _ => Vec::new(),
  }
}

/// Reads frames of `kind` from `stream` into `buffer` and returns the next one whole.
pub async fn next_frame(
  stream: &mut tokio::net::TcpStream,
  buffer: &mut Vec<u8>,
  kind: soulseek_proto::frame::FrameKind,
) -> Vec<u8> {
  use tokio::io::AsyncReadExt;
  loop {
    let split = soulseek_proto::frame::split(kind, buffer, &soulseek_proto::Limits::DEFAULT);
    if let Some((_, consumed)) = split.unwrap() {
      return buffer.drain(..consumed).collect();
    }
    let mut chunk = [0; 4096];
    let read = stream.read(&mut chunk).await.unwrap();
    assert!(read > 0, "the daemon closed the connection");
    buffer.extend_from_slice(&chunk[..read]);
  }
}

/// `username` as a room's member list has an online user.
pub fn member(username: &str) -> UserData {
  UserData {
    username: username.into(),
    status: UserPresence::Online,
    stats: UserStats::default(),
    slots_full: false,
    country_code: String::new(),
  }
}
