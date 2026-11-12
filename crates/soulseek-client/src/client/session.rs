//! The server session as one value: each stage holds what exists in it, and [`Stage::advance`]
//! is the one place that moves from one stage to the next, refusing an edge the session cannot
//! take from where it is.

use std::net::SocketAddr;

use soulseek_proto::Secret;

use crate::conn::MessageConn;
use crate::listener::Listener;
use crate::state::ClientState;

/// The server connection of a session that reached the server
pub(super) struct Link {
  pub(super) server: MessageConn,
  /// the server address as given to `connect`
  pub(super) address: String,
  /// what the address resolved to
  pub(super) ip_endpoint: SocketAddr,
}

/// Where the session is, with what exists there. The listener, when the options enable one,
/// lives from the connect until the disconnect takes it.
pub(super) enum Stage {
  /// no session; `reached` when the last one reached the server
  Disconnected {
    reached: bool,
  },
  Connecting {
    listener: Option<Listener>,
  },
  Connected {
    listener: Option<Listener>,
    link: Link,
  },
  /// the login of `username` with `password` was sent
  LoggingIn {
    listener: Option<Listener>,
    link: Link,
    username: String,
    password: Secret,
  },
  LoggedIn {
    listener: Option<Listener>,
    link: Link,
    username: String,
    password: Secret,
  },
  /// the session is being torn down; `reached` when it had reached the server
  Disconnecting {
    reached: bool,
  },
}

/// A move of the session from one stage to the next, with what the next stage adds
pub(super) enum Edge {
  /// from disconnected to connecting
  Connect,
  /// from connecting to connected, once the TCP connection is up
  Reach(Link),
  /// from connected to logging in, once the login of `username` with `password` is about to be
  /// sent
  LogIn { username: String, password: Secret },
  /// from logging in to logged in, once the server accepted the login
  Accept,
  /// from connecting, connected, logging in or logged in to disconnecting
  Disconnect,
  /// from disconnecting to disconnected
  Finish,
}

/// What a disconnect takes out of the session, to be closed outside its lock
#[derive(Default)]
pub(super) struct Released {
  pub(super) listener: Option<Listener>,
  pub(super) server: Option<MessageConn>,
}

impl Stage {
  pub(super) fn state(&self) -> ClientState {
    match self {
      Self::Disconnected { .. } => ClientState::Disconnected,
      Self::Connecting { .. } => ClientState::Connecting,
      Self::Connected { .. } => ClientState::Connected,
      Self::LoggingIn { .. } => ClientState::LoggingIn,
      Self::LoggedIn { .. } => ClientState::LoggedIn,
      Self::Disconnecting { .. } => ClientState::Disconnecting,
    }
  }

  /// the session reached the server: it did, or the one that ended last did
  pub(super) fn reached(&self) -> bool {
    match self {
      Self::Disconnected { reached } | Self::Disconnecting { reached } => *reached,
      Self::Connecting { .. } => false,
      Self::Connected { .. } | Self::LoggingIn { .. } | Self::LoggedIn { .. } => true,
    }
  }

  /// the server connection, once the session reached the server and until it is torn down
  pub(super) fn link(&self) -> Option<&Link> {
    match self {
      Self::Connected { link, .. } | Self::LoggingIn { link, .. } | Self::LoggedIn { link, .. } => {
        Some(link)
      }
      Self::Disconnected { .. } | Self::Connecting { .. } | Self::Disconnecting { .. } => None,
    }
  }

  /// our username, while logged in
  pub(super) fn username(&self) -> Option<&str> {
    match self {
      Self::LoggedIn { username, .. } => Some(username),
      _ => None,
    }
  }

  /// the listener's place, while the session holds one; `None` disconnecting or disconnected
  pub(super) fn listener(&mut self) -> Option<&mut Option<Listener>> {
    match self {
      Self::Connecting { listener }
      | Self::Connected { listener, .. }
      | Self::LoggingIn { listener, .. }
      | Self::LoggedIn { listener, .. } => Some(listener),
      Self::Disconnected { .. } | Self::Disconnecting { .. } => None,
    }
  }

  /// Takes `edge` from this stage, returning what a disconnect releases, or refuses it with the
  /// state it was refused in and leaves the stage as it was.
  pub(super) fn advance(&mut self, edge: Edge) -> Result<Released, ClientState> {
    let current = std::mem::replace(self, Self::Disconnected { reached: false });
    let (next, released) = match (current, edge) {
      (Self::Disconnected { .. }, Edge::Connect) => {
        (Self::Connecting { listener: None }, Released::default())
      }
      (Self::Connecting { listener }, Edge::Reach(link)) => {
        (Self::Connected { listener, link }, Released::default())
      }
      (Self::Connected { listener, link }, Edge::LogIn { username, password }) => (
        Self::LoggingIn {
          listener,
          link,
          username,
          password,
        },
        Released::default(),
      ),
      (
        Self::LoggingIn {
          listener,
          link,
          username,
          password,
        },
        Edge::Accept,
      ) => (
        Self::LoggedIn {
          listener,
          link,
          username,
          password,
        },
        Released::default(),
      ),
      (Self::Connecting { listener }, Edge::Disconnect) => (
        Self::Disconnecting { reached: false },
        Released {
          listener,
          server: None,
        },
      ),
      (
        Self::Connected { listener, link }
        | Self::LoggingIn { listener, link, .. }
        | Self::LoggedIn { listener, link, .. },
        Edge::Disconnect,
      ) => (
        Self::Disconnecting { reached: true },
        Released {
          listener,
          server: Some(link.server),
        },
      ),
      (Self::Disconnecting { reached }, Edge::Finish) => {
        (Self::Disconnected { reached }, Released::default())
      }
      (current, _) => {
        let state = current.state();
        *self = current;
        return Err(state);
      }
    };
    *self = next;
    Ok(released)
  }
}

#[cfg(test)]
mod tests {
  use soulseek_proto::Limits;
  use soulseek_proto::frame::FrameKind;

  use super::*;
  use crate::conn::tests::pair;
  use crate::options::ConnectionOptions;

  async fn link() -> Link {
    let (conn, _remote) = pair(ConnectionOptions::default()).await;
    let ip_endpoint = conn.handle().remote();
    Link {
      server: conn
        .into_message(FrameKind::Server, Limits::DEFAULT)
        .unwrap(),
      address: "server".into(),
      ip_endpoint,
    }
  }

  fn edge_name(edge: &Edge) -> &'static str {
    match edge {
      Edge::Connect => "Connect",
      Edge::Reach(_) => "Reach",
      Edge::LogIn { .. } => "LogIn",
      Edge::Accept => "Accept",
      Edge::Disconnect => "Disconnect",
      Edge::Finish => "Finish",
    }
  }

  /// every edge, in the order a session takes them
  async fn edges() -> Vec<Edge> {
    vec![
      Edge::Connect,
      Edge::Reach(link().await),
      Edge::LogIn {
        username: "alice".into(),
        password: Secret::new("secret"),
      },
      Edge::Accept,
      Edge::Disconnect,
      Edge::Finish,
    ]
  }

  /// a session moved along `path` from disconnected
  async fn at(path: &[&str]) -> Stage {
    let mut stage = Stage::Disconnected { reached: false };
    for name in path {
      let edge = edges()
        .await
        .into_iter()
        .find(|edge| edge_name(edge) == *name)
        .unwrap();
      assert!(stage.advance(edge).is_ok(), "{name}");
    }
    stage
  }

  #[tokio::test]
  async fn the_session_takes_only_the_edges_of_its_stage() {
    let paths: [&[&str]; 6] = [
      &[],
      &["Connect"],
      &["Connect", "Reach"],
      &["Connect", "Reach", "LogIn"],
      &["Connect", "Reach", "LogIn", "Accept"],
      &["Connect", "Reach", "Disconnect"],
    ];
    let allowed = [
      (ClientState::Disconnected, vec!["Connect"]),
      (ClientState::Connecting, vec!["Reach", "Disconnect"]),
      (ClientState::Connected, vec!["LogIn", "Disconnect"]),
      (ClientState::LoggingIn, vec!["Accept", "Disconnect"]),
      (ClientState::LoggedIn, vec!["Disconnect"]),
      (ClientState::Disconnecting, vec!["Finish"]),
    ];
    for (path, (state, allowed)) in paths.into_iter().zip(allowed) {
      for edge in edges().await {
        let name = edge_name(&edge);
        let mut stage = at(path).await;
        assert_eq!(stage.state(), state);
        match stage.advance(edge) {
          Ok(_) => assert!(allowed.contains(&name), "{state} took {name}"),
          Err(refused) => {
            assert!(!allowed.contains(&name), "{state} refused {name}");
            assert_eq!(refused, state);
            assert_eq!(stage.state(), state, "a refused edge leaves the stage");
          }
        }
      }
    }
  }

  #[tokio::test]
  async fn a_disconnect_releases_the_connection_and_forgets_the_login() {
    let mut stage = at(&["Connect", "Reach", "LogIn", "Accept"]).await;
    assert_eq!(stage.username(), Some("alice"));
    let released = stage.advance(Edge::Disconnect).unwrap();
    assert!(released.server.is_some());
    assert!(stage.link().is_none());
    assert_eq!(stage.username(), None);
    assert!(stage.listener().is_none());
    assert!(stage.reached());
    stage.advance(Edge::Finish).unwrap();
    assert!(stage.reached(), "the ended session's reach is kept");

    let mut stage = at(&["Connect"]).await;
    let released = stage.advance(Edge::Disconnect).unwrap();
    assert!(released.server.is_none());
    assert!(!stage.reached());
  }
}
