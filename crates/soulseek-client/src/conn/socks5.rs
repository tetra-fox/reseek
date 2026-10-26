//! The SOCKS5 handshake on a socket already connected to the proxy: method selection, the
//! optional username/password step, and the CONNECT request to an IPv4 destination. Every reply
//! is read by its exact length, so replies split across segments are handled.

use std::net::SocketAddrV4;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{ClientError, ConnectionErrorKind};
use crate::options::ProxyCredentials;

const VERSION: u8 = 5;
const AUTH_VERSION: u8 = 1;
const NO_AUTHENTICATION: u8 = 0;
const USERNAME_PASSWORD: u8 = 2;
const NO_ACCEPTABLE_METHOD: u8 = 0xff;
const CONNECT: u8 = 1;
const IPV4: u8 = 1;
const DOMAIN: u8 = 3;
const IPV6: u8 = 4;

/// The reply of a proxy whose destination refused the connection.
const CONNECTION_REFUSED: u8 = 5;

/// Asks the proxy on `stream` to connect to `destination`. Failures are proxy errors; one whose
/// destination refused the connection carries a refused connect as its cause, as a direct
/// connect would.
pub(crate) async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
  stream: &mut S,
  destination: SocketAddrV4,
  credentials: Option<&ProxyCredentials>,
) -> Result<(), ClientError> {
  negotiate(stream, destination, credentials)
    .await
    .map_err(|error| match error {
      Failure::Proxy(message) => proxy_error(message),
      Failure::Refused(message) => ClientError::connection_from(
        ConnectionErrorKind::Proxy,
        message,
        std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
      ),
      Failure::Io(error) => ClientError::connection_from(
        ConnectionErrorKind::Proxy,
        format!("Failed to connect to proxy: {error}"),
        error,
      ),
    })
}

enum Failure {
  Proxy(String),
  /// the destination refused the proxy's connection
  Refused(String),
  Io(std::io::Error),
}

impl From<std::io::Error> for Failure {
  fn from(error: std::io::Error) -> Self {
    Self::Io(error)
  }
}

fn proxy_error(message: String) -> ClientError {
  ClientError::connection(ConnectionErrorKind::Proxy, message)
}

async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
  stream: &mut S,
  destination: SocketAddrV4,
  credentials: Option<&ProxyCredentials>,
) -> Result<(), Failure> {
  let methods: &[u8] = match credentials {
    Some(_) => &[VERSION, 2, NO_AUTHENTICATION, USERNAME_PASSWORD],
    None => &[VERSION, 1, NO_AUTHENTICATION],
  };
  stream.write_all(methods).await?;

  let [version, method] = read_array(stream).await?;
  check_version(version)?;
  match method {
    NO_AUTHENTICATION => {}
    USERNAME_PASSWORD => {
      let Some(credentials) = credentials else {
        return Err(Failure::Proxy(
          "Server requests authorization but none was provided".into(),
        ));
      };
      authenticate(stream, credentials).await?;
    }
    NO_ACCEPTABLE_METHOD => {
      return Err(Failure::Proxy(
        "Server does not support the specified authentication method(s)".into(),
      ));
    }
    other => {
      return Err(Failure::Proxy(format!(
        "Unknown auth METHOD response from server: {other}"
      )));
    }
  }

  let mut request = vec![VERSION, CONNECT, 0, IPV4];
  request.extend(destination.ip().octets());
  request.extend(destination.port().to_be_bytes());
  stream.write_all(&request).await?;

  let [version, reply, _reserved, address_type] = read_array(stream).await?;
  check_version(version)?;
  if reply != 0 {
    let message = format!("SOCKS connection failed: {}", reply_text(reply));
    return Err(if reply == CONNECTION_REFUSED {
      Failure::Refused(message)
    } else {
      Failure::Proxy(message)
    });
  }
  // the address the proxy bound for us is read off the stream and not used
  let address_len = match address_type {
    IPV4 => 4,
    IPV6 => 16,
    DOMAIN => match stream.read_u8().await? {
      0xff => return Err(Failure::Proxy("Invalid domain name".into())),
      len => usize::from(len),
    },
    other => {
      return Err(Failure::Proxy(format!(
        "Unknown SOCKS Address type (expected: 1, 3 or 4, received: {other})"
      )));
    }
  };
  let mut bound = vec![0; address_len + 2];
  stream.read_exact(&mut bound).await?;
  Ok(())
}

async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
  stream: &mut S,
  credentials: &ProxyCredentials,
) -> Result<(), Failure> {
  let username = credentials.username.as_bytes();
  let password = credentials.password.expose().as_bytes();
  // option validation keeps both between 1 and 255 bytes, so each length fits its byte
  let mut request = vec![AUTH_VERSION, username.len() as u8];
  request.extend(username);
  request.push(password.len() as u8);
  request.extend(password);
  stream.write_all(&request).await?;

  let [version, status] = read_array(stream).await?;
  if version != AUTH_VERSION {
    return Err(Failure::Proxy(format!(
      "Invalid authentication subnegotiation version (expected: {AUTH_VERSION}, received: {version})"
    )));
  }
  if status != 0 {
    return Err(Failure::Proxy(format!(
      "Authentication failed: error code {status}"
    )));
  }
  Ok(())
}

async fn read_array<S: AsyncRead + Unpin, const N: usize>(
  stream: &mut S,
) -> Result<[u8; N], Failure> {
  let mut bytes = [0; N];
  stream.read_exact(&mut bytes).await?;
  Ok(bytes)
}

fn check_version(version: u8) -> Result<(), Failure> {
  if version != VERSION {
    return Err(Failure::Proxy(format!(
      "Invalid SOCKS version (expected: {VERSION}, received: {version})"
    )));
  }
  Ok(())
}

fn reply_text(reply: u8) -> String {
  match reply {
    1 => "General SOCKS server failure".into(),
    2 => "Connection not allowed by ruleset".into(),
    3 => "Network unreachable".into(),
    4 => "Host unreachable".into(),
    CONNECTION_REFUSED => "Connection refused".into(),
    6 => "TTL expired".into(),
    7 => "Command not supported".into(),
    8 => "Address type not supported".into(),
    other => format!("Unknown SOCKS error {other}"),
  }
}

#[cfg(test)]
mod tests {
  use std::net::Ipv4Addr;

  use soulseek_proto::Secret;
  use tokio::io::{DuplexStream, duplex};

  use super::*;

  const DESTINATION: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(10, 1, 2, 3), 2271);

  fn credentials() -> ProxyCredentials {
    ProxyCredentials {
      username: "user".into(),
      password: Secret::new("pass"),
    }
  }

  /// a proxy that expects `expect` and answers with `replies`, one byte per write so every read
  /// on the client side has to gather its bytes
  async fn proxy(mut stream: DuplexStream, exchanges: Vec<(Vec<u8>, Vec<u8>)>) {
    for (expect, reply) in exchanges {
      let mut received = vec![0; expect.len()];
      stream.read_exact(&mut received).await.unwrap();
      assert_eq!(received, expect);
      for byte in reply {
        stream.write_all(&[byte]).await.unwrap();
        tokio::task::yield_now().await;
      }
    }
  }

  fn connect_request() -> Vec<u8> {
    vec![5, 1, 0, 1, 10, 1, 2, 3, 0x08, 0xdf]
  }

  async fn run(
    credentials: Option<ProxyCredentials>,
    exchanges: Vec<(Vec<u8>, Vec<u8>)>,
  ) -> Result<(), ClientError> {
    let (mut client, server) = duplex(64);
    let proxy = tokio::spawn(proxy(server, exchanges));
    let result = handshake(&mut client, DESTINATION, credentials.as_ref()).await;
    drop(client);
    proxy.await.unwrap();
    result
  }

  fn proxy_message(result: Result<(), ClientError>) -> String {
    match result {
      Err(ClientError::Connection {
        kind: ConnectionErrorKind::Proxy,
        message,
        ..
      }) => message,
      other => panic!("expected a proxy error, got {other:?}"),
    }
  }

  #[tokio::test]
  async fn connects_without_authentication_from_split_replies() {
    run(
      None,
      vec![
        (vec![5, 1, 0], vec![5, 0]),
        (
          connect_request(),
          vec![5, 0, 0, 1, 127, 0, 0, 1, 0x1f, 0x90],
        ),
      ],
    )
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn authenticates_and_reads_a_domain_bound_address() {
    run(
      Some(credentials()),
      vec![
        (vec![5, 2, 0, 2], vec![5, 2]),
        ([&[1, 4][..], b"user", &[4], b"pass"].concat(), vec![1, 0]),
        (
          connect_request(),
          [&[5, 0, 0, 3, 5][..], b"proxy", &[0x1f, 0x90]].concat(),
        ),
      ],
    )
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn refused_authentication() {
    let result = run(
      Some(credentials()),
      vec![
        (vec![5, 2, 0, 2], vec![5, 2]),
        ([&[1, 4][..], b"user", &[4], b"pass"].concat(), vec![1, 1]),
      ],
    )
    .await;
    assert_eq!(proxy_message(result), "Authentication failed: error code 1");
  }

  #[tokio::test]
  async fn authentication_requested_without_credentials() {
    let result = run(None, vec![(vec![5, 1, 0], vec![5, 2])]).await;
    assert_eq!(
      proxy_message(result),
      "Server requests authorization but none was provided"
    );
  }

  #[tokio::test]
  async fn wrong_version_and_no_acceptable_method() {
    let result = run(None, vec![(vec![5, 1, 0], vec![4, 0])]).await;
    assert_eq!(
      proxy_message(result),
      "Invalid SOCKS version (expected: 5, received: 4)"
    );
    let result = run(None, vec![(vec![5, 1, 0], vec![5, 0xff])]).await;
    assert_eq!(
      proxy_message(result),
      "Server does not support the specified authentication method(s)"
    );
  }

  #[tokio::test]
  async fn connect_failure_names_the_reply() {
    let result = run(
      None,
      vec![
        (vec![5, 1, 0], vec![5, 0]),
        (connect_request(), vec![5, 5, 0, 1]),
      ],
    )
    .await;
    assert_eq!(
      proxy_message(result),
      "SOCKS connection failed: Connection refused"
    );
  }

  #[tokio::test]
  async fn proxy_closing_early_is_a_proxy_error() {
    let result = run(None, vec![(vec![5, 1, 0], vec![5])]).await;
    assert!(proxy_message(result).starts_with("Failed to connect to proxy: "));
  }
}
