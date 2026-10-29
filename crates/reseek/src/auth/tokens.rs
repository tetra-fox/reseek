//! API tokens: `slsk_` and 40 random letters and digits, of which only a SHA-256 is kept. A
//! token is found by the first characters of its secret, checked in constant time, and may be
//! limited to some networks, expire, or be revoked.

use std::net::IpAddr;

use api_types::auth::tokens::Token;
use api_types::auth::{Principal, Role, Via};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use rand::distr::Alphanumeric;
use rusqlite::{OptionalExtension, Row, params};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::warn;
use uuid::Uuid;

use super::store::StoredRole;
use super::{TOKEN_PREFIX, networks};
use crate::db::{Database, DbError, read_timestamp, timestamp};

/// How many random characters follow `slsk_`.
const SECRET_CHARS: usize = 40;

/// How many characters after `slsk_` are kept in the clear to find and tell tokens apart.
const PREFIX_CHARS: usize = 8;

/// How often a token's last use is written at most.
const USE_WRITES: Duration = Duration::minutes(1);

/// Why a presented token was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
  /// no token has this secret
  #[error("unknown API token")]
  Unknown,
  /// the token's expiry passed
  #[error("token_expired")]
  Expired,
  /// the token was revoked
  #[error("token_revoked")]
  Revoked,
  /// the token may not be used from the client's address
  #[error("API token is not allowed from {0}")]
  Address(IpAddr),
}

/// A token as stored, without its secret.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredToken {
  /// the token's id
  pub id: Uuid,
  /// its name
  pub name: String,
  /// the first characters of its secret
  pub prefix: String,
  /// what it may do
  pub role: Role,
  /// the subject that created it
  pub owner: String,
  /// the networks it may be used from, comma-separated, `None` for anywhere
  pub cidr: Option<String>,
  /// when it was created
  pub created_at: DateTime<Utc>,
  /// when it stops working
  pub expires_at: Option<DateTime<Utc>>,
  /// when it was last used, to within a minute
  pub last_used_at: Option<DateTime<Utc>>,
  /// where it was last used from
  pub last_used_ip: Option<String>,
  /// when it was revoked
  pub revoked_at: Option<DateTime<Utc>>,
}

impl StoredToken {
  /// The token as the API shows it.
  pub fn to_api(&self) -> Token {
    Token {
      id: self.id,
      name: self.name.clone(),
      prefix: self.prefix.clone(),
      role: self.role,
      owner: self.owner.clone(),
      cidr: self.cidr.clone(),
      created_at: self.created_at,
      expires_at: self.expires_at,
      last_used_at: self.last_used_at,
      last_used_ip: self.last_used_ip.clone(),
    }
  }

  /// The principal a request with this token acts as.
  pub fn principal(&self) -> Principal {
    Principal {
      subject: format!("token:{}", self.id),
      name: self.name.clone(),
      role: self.role,
      via: Via::ApiToken { id: self.id },
    }
  }
}

/// What a new token is.
pub struct NewToken {
  /// its name
  pub name: String,
  /// what it may do
  pub role: Role,
  /// who creates it
  pub owner: String,
  /// the networks it may be used from
  pub cidr: Option<String>,
  /// when it stops working
  pub expires_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, name, prefix, role, owner_subject, cidr, created_at, expires_at, \
                       last_used_at, last_used_ip, revoked_at";

fn read_row(row: &Row<'_>) -> rusqlite::Result<StoredToken> {
  let time = |index: usize| -> rusqlite::Result<Option<DateTime<Utc>>> {
    row
      .get::<_, Option<String>>(index)?
      .as_deref()
      .map(read_timestamp)
      .transpose()
  };
  let id: String = row.get(0)?;
  Ok(StoredToken {
    id: id.parse().map_err(|e: uuid::Error| {
      rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })?,
    name: row.get(1)?,
    prefix: row.get(2)?,
    role: row.get::<_, StoredRole>(3)?.0,
    owner: row.get(4)?,
    cidr: row.get(5)?,
    created_at: read_timestamp(&row.get::<_, String>(6)?)?,
    expires_at: time(7)?,
    last_used_at: time(8)?,
    last_used_ip: row.get(9)?,
    revoked_at: time(10)?,
  })
}

fn hash(secret: &str) -> Vec<u8> {
  Sha256::digest(secret.as_bytes()).to_vec()
}

/// The characters after `slsk_` that find the token, `None` when `presented` can not be one.
fn prefix(presented: &str) -> Option<&str> {
  let secret = presented.strip_prefix(TOKEN_PREFIX)?;
  let well_formed =
    secret.len() == SECRET_CHARS && secret.bytes().all(|b| b.is_ascii_alphanumeric());
  well_formed.then(|| &secret[..PREFIX_CHARS])
}

/// Creates a token and returns it with its secret, which is not kept.
pub async fn create(db: &Database, new: NewToken) -> Result<(StoredToken, String), DbError> {
  let random: String = rand::rng()
    .sample_iter(&Alphanumeric)
    .take(SECRET_CHARS)
    .map(char::from)
    .collect();
  let secret = format!("{TOKEN_PREFIX}{random}");
  let token = StoredToken {
    id: Uuid::new_v4(),
    name: new.name,
    prefix: random[..PREFIX_CHARS].to_string(),
    role: new.role,
    owner: new.owner,
    cidr: new.cidr,
    created_at: Utc::now(),
    expires_at: new.expires_at,
    last_used_at: None,
    last_used_ip: None,
    revoked_at: None,
  };
  let row = token.clone();
  let secret_hash = hash(&secret);
  db.write(move |conn| {
    conn.execute(
      "INSERT INTO api_tokens (id, name, prefix, secret_hash, role, owner_subject, cidr, \
       created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
      params![
        row.id.to_string(),
        row.name,
        row.prefix,
        secret_hash,
        StoredRole(row.role),
        row.owner,
        row.cidr,
        timestamp(row.created_at),
        row.expires_at.map(timestamp),
      ],
    )
  })
  .await?;
  Ok((token, secret))
}

/// The token whose secret is `presented`, if it may be used from `ip` now. Its last use is
/// recorded when the record is more than a minute old.
pub async fn check(
  db: &Database,
  presented: &str,
  ip: IpAddr,
) -> Result<Result<StoredToken, Refusal>, DbError> {
  let Some(prefix) = prefix(presented).map(str::to_string) else {
    return Ok(Err(Refusal::Unknown));
  };
  let wanted = hash(presented);
  let candidates = db
    .read(move |conn| {
      let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS}, secret_hash FROM api_tokens WHERE prefix = ?1"
      ))?;
      statement
        .query_map([prefix], |row| {
          Ok((read_row(row)?, row.get::<_, Vec<u8>>(11)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
    })
    .await?;
  let mut found = None;
  for (token, secret_hash) in candidates {
    let matches: bool = secret_hash.ct_eq(&wanted).into();
    if matches {
      found = Some(token);
    }
  }
  let Some(token) = found else {
    return Ok(Err(Refusal::Unknown));
  };
  let now = Utc::now();
  if token.revoked_at.is_some() {
    return Ok(Err(Refusal::Revoked));
  }
  if token.expires_at.is_some_and(|at| at <= now) {
    return Ok(Err(Refusal::Expired));
  }
  if let Some(cidr) = &token.cidr
    && !networks::ranges(std::slice::from_ref(cidr))
      .iter()
      .any(|range| range.contains(ip))
  {
    return Ok(Err(Refusal::Address(ip)));
  }
  if token.last_used_at.is_none_or(|at| now - at >= USE_WRITES) {
    let db = db.clone();
    let id = token.id.to_string();
    tokio::spawn(async move {
      let written = db
        .write(move |conn| {
          conn.execute(
            "UPDATE api_tokens SET last_used_at = ?1, last_used_ip = ?2 WHERE id = ?3",
            params![timestamp(now), ip.to_string(), id],
          )
        })
        .await;
      if let Err(e) = written {
        warn!(error = %e, "a token's last use was not recorded");
      }
    });
  }
  Ok(Ok(token))
}

/// The tokens not revoked, of `owner` or of everyone, oldest first.
pub async fn list(db: &Database, owner: Option<String>) -> Result<Vec<StoredToken>, DbError> {
  db.read(move |conn| {
    let mut statement = conn.prepare(&format!(
      "SELECT {COLUMNS} FROM api_tokens WHERE revoked_at IS NULL AND (?1 IS NULL OR \
       owner_subject = ?1) ORDER BY created_at"
    ))?;
    statement
      .query_map([owner], read_row)?
      .collect::<rusqlite::Result<Vec<_>>>()
  })
  .await
}

/// The token `id`, when it is not revoked.
pub async fn get(db: &Database, id: Uuid) -> Result<Option<StoredToken>, DbError> {
  db.read(move |conn| {
    conn
      .query_row(
        &format!("SELECT {COLUMNS} FROM api_tokens WHERE id = ?1 AND revoked_at IS NULL"),
        [id.to_string()],
        read_row,
      )
      .optional()
  })
  .await
}

/// Renames the token `id` or changes its expiry, each when given.
pub async fn update(
  db: &Database,
  id: Uuid,
  name: Option<String>,
  expires_at: Option<DateTime<Utc>>,
) -> Result<Option<StoredToken>, DbError> {
  db.write(move |conn| {
    conn.execute(
      "UPDATE api_tokens SET name = coalesce(?1, name), expires_at = coalesce(?2, expires_at) \
       WHERE id = ?3 AND revoked_at IS NULL",
      params![name, expires_at.map(timestamp), id.to_string()],
    )?;
    conn
      .query_row(
        &format!("SELECT {COLUMNS} FROM api_tokens WHERE id = ?1 AND revoked_at IS NULL"),
        [id.to_string()],
        read_row,
      )
      .optional()
  })
  .await
}

/// Revokes the token `id`; whether it was not revoked already.
pub async fn revoke(db: &Database, id: Uuid) -> Result<bool, DbError> {
  db.write(move |conn| {
    conn.execute(
      "UPDATE api_tokens SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
      params![timestamp(Utc::now()), id.to_string()],
    )
  })
  .await
  .map(|changed| changed == 1)
}

/// Deletes the tokens revoked or expired before `before`; how many.
pub async fn prune(db: &Database, before: DateTime<Utc>) -> Result<usize, DbError> {
  db.write(move |conn| {
    conn.execute(
      "DELETE FROM api_tokens WHERE revoked_at < ?1 OR expires_at < ?1",
      [timestamp(before)],
    )
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_well_formed_secrets_are_looked_up() {
    let secret = format!("slsk_{}", "a1B2c3D4".repeat(5));
    assert_eq!(prefix(&secret), Some("a1B2c3D4"));
    assert_eq!(prefix("slsk_short"), None);
    assert_eq!(prefix(&format!("slsk_{}!", "a".repeat(39))), None);
    assert_eq!(prefix(&format!("other_{}", "a".repeat(40))), None);
  }
}
