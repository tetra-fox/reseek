//! The relay socket an agent keeps open to its controller, opened with the client's API key or
//! token and the relay protocol. It carries one [`RelayEnvelope`] per text frame.

use api_types::relay::RelayEnvelope;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::{HeaderMap, HeaderValue};

use super::{Client, Credential, Error, Result};

/// The relay socket, as an agent holds it.
pub struct RelaySocket {
  socket: reseek_agent::Socket,
}

/// What the controller sent on the relay socket.
#[derive(Debug, PartialEq)]
pub enum RelayReceived {
  /// a message
  Message(RelayEnvelope),
  /// the close frame, with its code when it had one
  Closed(Option<u16>),
}

impl Client {
  /// `GET /relay/ws`: opens the relay socket with the client's API key or token, dialed as the
  /// agent dials it.
  pub async fn relay_socket(&self) -> Result<RelaySocket> {
    let url = url::Url::parse(&format!("{}/api/v0/relay/ws", self.base))
      .map_err(|e| Error::Base(e.to_string()))?;
    let header =
      |value: &str| HeaderValue::from_str(value).map_err(|e| Error::Socket(e.to_string()));
    let mut headers = HeaderMap::new();
    match &self.credential {
      Some(Credential::ApiKey(key)) => {
        headers.insert("x-api-key", header(key)?);
      }
      Some(Credential::Bearer(token)) => {
        headers.insert("authorization", header(&format!("Bearer {token}"))?);
      }
      None => {}
    }
    let socket = reseek_agent::open_socket(&url, &self.tls, headers)
      .await
      .map_err(Error::Socket)?;
    Ok(RelaySocket { socket })
  }
}

impl RelaySocket {
  /// Sends `envelope` as a text frame.
  pub async fn send(&mut self, envelope: &RelayEnvelope) -> Result<()> {
    let text = serde_json::to_string(envelope)?;
    self
      .socket
      .send(Message::Text(text.into()))
      .await
      .map_err(|e| Error::Socket(e.to_string()))
  }

  /// The next message or the close, skipping pings; `Closed(None)` when the socket ended
  /// without a close frame.
  pub async fn receive(&mut self) -> Result<RelayReceived> {
    loop {
      match self.socket.next().await {
        Some(Ok(Message::Text(text))) => {
          return Ok(RelayReceived::Message(serde_json::from_str(text.as_str())?));
        }
        Some(Ok(Message::Close(frame))) => {
          return Ok(RelayReceived::Closed(frame.map(|f| u16::from(f.code))));
        }
        Some(Ok(_)) => {}
        Some(Err(e)) => return Err(Error::Socket(e.to_string())),
        None => return Ok(RelayReceived::Closed(None)),
      }
    }
  }
}
