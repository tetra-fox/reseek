//! Local accounts: the built-in administrator and the extra accounts of
//! `web.authentication.local.users`, each checked against an argon2id hash. A plain password
//! from the configuration is hashed when the accounts are loaded, so every check takes the same
//! path, and a username that has no account is checked against a stand-in hash, so the time a
//! check takes does not tell whether the account exists. Only a few checks run at once, since
//! each one holds about 19 MiB while it hashes.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use api_types::auth::Role;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::warn;

use crate::config::{Authentication, LocalUser};
use crate::crypto;
use crate::sync::lock;

/// A local account that a password matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
  /// the username
  pub username: String,
  /// what it may do
  pub role: Role,
}

impl Account {
  /// The subject its sessions act as.
  pub fn subject(&self) -> String {
    format!("local:{}", self.username)
  }
}

/// The accounts with their password hashes, rebuilt when their configuration changes.
pub struct Accounts {
  loaded: Mutex<Option<(Settings, Arc<Hashed>)>>,
  /// one permit per check that may run at once, moved onto the blocking thread that hashes
  checks: Arc<Semaphore>,
}

impl Default for Accounts {
  fn default() -> Self {
    // one check per core; when the core count is unknown, one at a time
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    Self {
      loaded: Mutex::default(),
      checks: Arc::new(Semaphore::new(cores)),
    }
  }
}

/// Every password check that may run at once is running.
#[derive(Debug)]
pub struct Busy;

/// The part of the configuration the accounts come from.
#[derive(Clone, PartialEq)]
struct Settings {
  username: String,
  password: String,
  password_hash: Option<String>,
  users: BTreeMap<String, LocalUser>,
}

/// Every account with the PHC string its password is checked against.
struct Hashed {
  built_in: (String, String),
  users: BTreeMap<String, (String, Role)>,
}

impl Accounts {
  /// The account `username` with the password `password`, when there is one. Hashing runs on
  /// a blocking thread; [`Busy`] when as many checks as may run at once are running.
  pub async fn check(
    &self,
    authentication: &Authentication,
    username: String,
    password: String,
  ) -> Result<Option<Account>, Busy> {
    // the semaphore is never closed, so the only error is that no permit is free
    let permit = self.checks.clone().try_acquire_owned().map_err(|_| Busy)?;
    let Some((hashed, permit)) = self.hashed(authentication, permit).await else {
      return Ok(None);
    };
    let result = tokio::task::spawn_blocking(move || {
      let _permit = permit;
      hashed.check(&username, &password)
    })
    .await;
    match result {
      Ok(account) => Ok(account),
      Err(e) => {
        warn!(error = %e, "a password check did not finish");
        Ok(None)
      }
    }
  }

  /// The accounts for `authentication`, hashed again when it changed. `permit` is held by the
  /// blocking thread while it hashes, and handed back.
  async fn hashed(
    &self,
    authentication: &Authentication,
    permit: OwnedSemaphorePermit,
  ) -> Option<(Arc<Hashed>, OwnedSemaphorePermit)> {
    let settings = Settings {
      username: authentication.username.clone(),
      password: authentication.password.clone(),
      password_hash: authentication.password_hash.clone(),
      users: authentication.local.users.clone(),
    };
    if let Some((loaded, hashed)) = &*self.lock()
      && *loaded == settings
    {
      return Some((hashed.clone(), permit));
    }
    let built = settings.clone();
    let (hashed, permit) =
      match tokio::task::spawn_blocking(move || (Hashed::new(&built), permit)).await {
        Ok((hashed, permit)) => (Arc::new(hashed), permit),
        Err(e) => {
          warn!(error = %e, "local accounts were not loaded");
          return None;
        }
      };
    *self.lock() = Some((settings, hashed.clone()));
    Some((hashed, permit))
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Settings, Arc<Hashed>)>> {
    lock(&self.loaded)
  }
}

impl Hashed {
  fn new(settings: &Settings) -> Self {
    let built_in = settings
      .password_hash
      .clone()
      .unwrap_or_else(|| hash(&settings.password));
    let users = settings
      .users
      .iter()
      .filter_map(|(name, user)| {
        let phc = match (&user.password_hash, &user.password) {
          (Some(phc), _) => phc.clone(),
          (None, Some(plain)) => hash(plain),
          // refused when the configuration loaded
          (None, None) => return None,
        };
        Some((name.clone(), (phc, user.role.into())))
      })
      .collect();
    Self {
      built_in: (settings.username.clone(), built_in),
      users,
    }
  }

  fn check(&self, username: &str, password: &str) -> Option<Account> {
    let wanted = Sha256::digest(username.as_bytes());
    let is_built_in: bool = Sha256::digest(self.built_in.0.as_bytes())
      .ct_eq(&wanted)
      .into();
    let (phc, account) = if is_built_in {
      (
        self.built_in.1.as_str(),
        Some(Account {
          username: username.to_string(),
          role: Role::Administrator,
        }),
      )
    } else {
      match self.users.get(username) {
        Some((phc, role)) => (
          phc.as_str(),
          Some(Account {
            username: username.to_string(),
            role: *role,
          }),
        ),
        None => (stand_in(), None),
      }
    };
    let matches = verify(phc, password);
    account.filter(|_| matches)
  }
}

/// Whether `password` is the one `phc` was made from.
fn verify(phc: &str, password: &str) -> bool {
  match PasswordHash::new(phc) {
    Ok(parsed) => Argon2::default()
      .verify_password(password.as_bytes(), &parsed)
      .is_ok(),
    Err(e) => {
      // checked when the configuration loaded, so this is a bug if it happens
      tracing::error!(error = %e, "a password hash does not parse");
      false
    }
  }
}

/// A hash no password is checked against successfully, for usernames without an account.
fn stand_in() -> &'static str {
  static STAND_IN: OnceLock<String> = OnceLock::new();
  STAND_IN.get_or_init(|| hash(&crypto::base62(&crypto::random_bytes(16))))
}

/// `password` as an argon2id PHC string with a random salt.
pub fn hash(password: &str) -> String {
  let salt = SaltString::encode_b64(&crypto::random_bytes(16)).expect("16 bytes make a valid salt");
  Argon2::default()
    .hash_password(password.as_bytes(), &salt)
    .expect("argon2 hashes any password with the default parameters")
    .to_string()
}

/// Whether `phc` is a password hash this daemon can check passwords against.
pub fn parses(phc: &str) -> Result<(), String> {
  let parsed = PasswordHash::new(phc).map_err(|e| e.to_string())?;
  match parsed.algorithm.as_str() {
    "argon2id" | "argon2i" | "argon2d" => Ok(()),
    other => Err(format!("{other} is not an argon2 hash")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::enums;

  #[tokio::test]
  async fn the_built_in_account_and_extra_accounts_check_their_passwords() {
    let mut authentication = Authentication {
      username: "admin".into(),
      password: "hunter2".into(),
      ..Authentication::default()
    };
    authentication.local.users.insert(
      "viewer".into(),
      LocalUser {
        password_hash: Some(hash("look")),
        password: None,
        role: enums::Role::ReadOnly,
      },
    );
    authentication.local.users.insert(
      "plain".into(),
      LocalUser {
        password_hash: None,
        password: Some("text".into()),
        role: enums::Role::ReadWrite,
      },
    );
    let accounts = Accounts::default();
    let check = async |user: &str, password: &str| {
      accounts
        .check(&authentication, user.to_string(), password.to_string())
        .await
        .expect("one check at a time is never busy")
    };
    assert_eq!(
      check("admin", "hunter2").await.unwrap().role,
      Role::Administrator
    );
    assert_eq!(check("viewer", "look").await.unwrap().role, Role::ReadOnly);
    assert_eq!(check("plain", "text").await.unwrap().role, Role::ReadWrite);
    assert!(check("admin", "wrong").await.is_none());
    assert!(
      check("Admin", "hunter2").await.is_none(),
      "usernames are exact"
    );
    assert!(check("nobody", "hunter2").await.is_none());
    assert!(parses(&hash("x")).is_ok());
    assert!(parses("$2b$12$abcdefghijklmnopqrstuv").is_err());
  }
}
