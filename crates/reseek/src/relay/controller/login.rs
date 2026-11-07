//! Checking an agent's login: the name it gives must be a configured agent's, it must connect
//! from one of that agent's networks, the challenge of its socket must still be valid, its
//! credential must be the one for that challenge, and its version must be on the controller's
//! release line. The checks run in that order and the first that fails is the reason the login
//! is refused, so only an agent that proved its secret learns that its version is refused.

use std::net::IpAddr;
use std::time::Duration;

use api_types::relay::LoginFailure;
use tokio::time::Instant;

use super::Controller;
use crate::config::RelayAgent;
use reseek_relay_proto::credential::{Key, Purpose};
use reseek_relay_proto::version;

/// The challenge a socket was sent.
pub(super) struct Challenge {
  /// the token the login must answer
  pub token: String,
  /// when it was sent
  pub issued: Instant,
}

/// What an agent's login says of it.
pub(super) struct Login<'a> {
  /// the address it connects from
  pub address: IpAddr,
  /// the credential for the challenge
  pub credential: &'a str,
  /// the version it says it is
  pub version: &'a str,
}

/// Checks `login` against the configured `agent` with its name, if any, and the `challenge` of
/// the socket, if it still has one, at `now`, for a controller of version `ours`; `key`
/// derives the agent's key from its secret. Returns that key when the login checks out.
pub(super) fn check(
  agent: Option<&RelayAgent>,
  login: &Login,
  challenge: Option<&Challenge>,
  lifetime: Duration,
  now: Instant,
  ours: &str,
  key: impl FnOnce(&str) -> Key,
) -> Result<Key, LoginFailure> {
  let (address, credential) = (login.address, login.credential);
  let agent = agent.ok_or(LoginFailure::UnknownAgent)?;
  if !Controller::address_allowed(agent, address) {
    return Err(LoginFailure::AddressNotAllowed);
  }
  let challenge = challenge
    .filter(|c| now.duration_since(c.issued) <= lifetime)
    .ok_or(LoginFailure::ChallengeExpired)?;
  // a configured agent has a secret, which loading the options checked
  let secret = agent.secret.as_deref().unwrap_or_default();
  let key = key(secret);
  if !key.verifies(Purpose::Login, &challenge.token, credential) {
    return Err(LoginFailure::BadCredential);
  }
  if !version::compatible(ours, login.version) {
    return Err(LoginFailure::IncompatibleVersion);
  }
  Ok(key)
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECRET: &str = "0123456789abcdef";
  const LIFETIME: Duration = Duration::from_secs(10);
  const OURS: &str = "0.4.2";

  fn agent(cidr: &str) -> RelayAgent {
    RelayAgent {
      instance_name: Some("nas".into()),
      secret: Some(SECRET.into()),
      cidr: cidr.into(),
    }
  }

  fn key(secret: &str) -> Key {
    Key::derive(secret, "nas")
  }

  #[tokio::test(start_paused = true)]
  async fn each_check_refuses_with_its_own_reason() {
    let home: IpAddr = "192.168.1.5".parse().unwrap();
    let issued = Instant::now();
    let challenge = Challenge {
      token: "abc".into(),
      issued,
    };
    let right = key(SECRET).credential(Purpose::Login, "abc");
    let lan = agent("192.168.1.0/24, 10.0.0.0/8");
    let login =
      |agent: Option<&RelayAgent>, challenge: Option<&Challenge>, now, credential: &str| {
        let login = Login {
          address: home,
          credential,
          version: OURS,
        };
        check(agent, &login, challenge, LIFETIME, now, OURS, key).map(drop)
      };
    assert_eq!(login(Some(&lan), Some(&challenge), issued, &right), Ok(()));
    assert_eq!(
      login(None, Some(&challenge), issued, &right),
      Err(LoginFailure::UnknownAgent)
    );
    assert_eq!(
      login(Some(&agent("10.0.0.0/8")), Some(&challenge), issued, &right),
      Err(LoginFailure::AddressNotAllowed)
    );
    assert_eq!(
      login(Some(&lan), None, issued, &right),
      Err(LoginFailure::ChallengeExpired),
      "a challenge used once is gone"
    );
    assert_eq!(
      login(
        Some(&lan),
        Some(&challenge),
        issued + LIFETIME + Duration::from_millis(1),
        &right
      ),
      Err(LoginFailure::ChallengeExpired),
      "a stale challenge"
    );
    let replayed = key(SECRET).credential(Purpose::Login, "an earlier challenge");
    assert_eq!(
      login(Some(&lan), Some(&challenge), issued, &replayed),
      Err(LoginFailure::BadCredential)
    );
    let wrong = Key::derive("another secret!!", "nas").credential(Purpose::Login, "abc");
    assert_eq!(
      login(Some(&lan), Some(&challenge), issued, &wrong),
      Err(LoginFailure::BadCredential)
    );
    let versioned = |version: &str, credential: &str| {
      let login = Login {
        address: home,
        credential,
        version,
      };
      check(
        Some(&lan),
        &login,
        Some(&challenge),
        LIFETIME,
        issued,
        OURS,
        key,
      )
      .map(drop)
    };
    assert_eq!(versioned("0.4.0", &right), Ok(()), "a patch apart");
    assert_eq!(
      versioned("0.5.0", &right),
      Err(LoginFailure::IncompatibleVersion)
    );
    assert_eq!(
      versioned("test", &wrong),
      Err(LoginFailure::BadCredential),
      "the version is checked once the agent proved its secret"
    );
  }
}
