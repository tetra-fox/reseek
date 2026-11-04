//! Checking an agent's login: the name it gives must be a configured agent's, it must connect
//! from one of that agent's networks, the challenge of its socket must still be valid, and its
//! credential must be the one for that challenge. The checks run in that order and the first
//! that fails is the reason the login is refused.

use std::net::IpAddr;
use std::time::Duration;

use api_types::relay::LoginFailure;
use tokio::time::Instant;

use super::Controller;
use crate::config::RelayAgent;
use reseek_relay_proto::credential::{Key, Purpose};

/// The challenge a socket was sent.
pub(super) struct Challenge {
  /// the token the login must answer
  pub token: String,
  /// when it was sent
  pub issued: Instant,
}

/// Checks the login of `name` with `credential` from `address` against the configured
/// `agent` with that name, if any, and the `challenge` of the socket, if it still has one, at
/// `now`; `key` derives the agent's key from its secret.
pub(super) fn check(
  agent: Option<&RelayAgent>,
  address: IpAddr,
  challenge: Option<&Challenge>,
  lifetime: Duration,
  now: Instant,
  credential: &str,
  key: impl FnOnce(&str) -> Key,
) -> Result<(), LoginFailure> {
  let agent = agent.ok_or(LoginFailure::UnknownAgent)?;
  if !Controller::address_allowed(agent, address) {
    return Err(LoginFailure::AddressNotAllowed);
  }
  let challenge = challenge
    .filter(|c| now.duration_since(c.issued) <= lifetime)
    .ok_or(LoginFailure::ChallengeExpired)?;
  // a configured agent has a secret, which loading the options checked
  let secret = agent.secret.as_deref().unwrap_or_default();
  if !key(secret).verifies(Purpose::Login, &challenge.token, credential) {
    return Err(LoginFailure::BadCredential);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECRET: &str = "0123456789abcdef";
  const LIFETIME: Duration = Duration::from_secs(10);

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
        check(agent, home, challenge, LIFETIME, now, credential, key)
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
  }
}
