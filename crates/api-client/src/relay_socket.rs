//! The relay socket an agent keeps open to its controller, opened with the client's API key and
//! the relay protocol. It carries one [`RelayEnvelope`] per text frame.

use api_types::relay::{RELAY_SUBPROTOCOL, RelayEnvelope};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use super::{Client, Credential, Error, Result};

/// A connection that is plain or TLS.
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// The relay socket, as an agent holds it.
pub struct RelaySocket {
  socket: WebSocketStream<Box<dyn Io>>,
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
  /// `GET /relay/ws`: opens the relay socket with the client's API key or token.
  pub async fn relay_socket(&self) -> Result<RelaySocket> {
    let base = url::Url::parse(&self.base).map_err(|e| Error::Base(e.to_string()))?;
    let secure = base.scheme() == "https";
    let host = base
      .host_str()
      .ok_or_else(|| Error::Base("no host".to_string()))?
      .trim_start_matches('[')
      .trim_end_matches(']')
      .to_string();
    let port = base
      .port_or_known_default()
      .ok_or_else(|| Error::Base("no port".to_string()))?;
    let tcp = TcpStream::connect((host.as_str(), port))
      .await
      .map_err(|e| Error::Socket(e.to_string()))?;
    let stream: Box<dyn Io> = if secure {
      let name =
        rustls::pki_types::ServerName::try_from(host).map_err(|e| Error::Socket(e.to_string()))?;
      Box::new(
        tokio_rustls::TlsConnector::from(self.tls.clone())
          .connect(name, tcp)
          .await
          .map_err(|e| Error::Socket(e.to_string()))?,
      )
    } else {
      Box::new(tcp)
    };
    let scheme = if secure { "wss" } else { "ws" };
    let rest = &self.base[self.base.find("://").map_or(0, |i| i + 3)..];
    let mut request = format!("{scheme}://{rest}/api/v0/relay/ws")
      .into_client_request()
      .map_err(|e| Error::Socket(e.to_string()))?;
    let headers = request.headers_mut();
    let header =
      |value: &str| HeaderValue::from_str(value).map_err(|e| Error::Socket(e.to_string()));
    match &self.credential {
      Some(Credential::ApiKey(key)) => {
        headers.insert("x-api-key", header(key)?);
      }
      Some(Credential::Bearer(token)) => {
        headers.insert("authorization", header(&format!("Bearer {token}"))?);
      }
      None => {}
    }
    headers.insert(
      "sec-websocket-protocol",
      HeaderValue::from_static(RELAY_SUBPROTOCOL),
    );
    let (socket, _) = tokio_tungstenite::client_async(request, stream)
      .await
      .map_err(|e| Error::Socket(e.to_string()))?;
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
