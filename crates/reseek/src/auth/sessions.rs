//! Browser sessions: kept in `auth.db` so they can be listed and revoked. The cookie holds the
//! session's id and an HMAC of it, so a forged id is refused before the database is read. A
//! session ends when it expires, when it goes unused for too long, or when it is revoked.

use api_types::auth::session::Session;
use api_types::auth::{Principal, Role, Via};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, Row, params};

use super::secret::Secret;
use super::store::StoredRole;
use crate::crypto;
use crate::db::{Database, DbError, read_timestamp, shifted, timestamp};

/// How often a session's last use is written at most, for a long idle limit; a shorter limit
/// writes it at a tenth of the limit, so the use recorded is never far enough behind to end a
/// session in use.
pub const SEEN_WRITES: Duration = Duration::minutes(5);

/// How many random bytes a session id has.
const ID_BYTES: usize = 32;

/// How a session was established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
  /// a local account's password login
  Local,
  /// a login at the OpenID Connect provider of this name
  Oidc(String),
  /// a trusted reverse proxy's forwarded identity
  Proxy,
}

impl Origin {
  /// The text stored for it: `local`, `oidc:<name>` or `proxy`.
  pub fn stored(&self) -> String {
    match self {
      Origin::Local => "local".to_string(),
      Origin::Oidc(name) => format!("oidc:{name}"),
      Origin::Proxy => "proxy".to_string(),
    }
  }

  fn parse(text: &str) -> rusqlite::Result<Self> {
    match text {
      "local" => Ok(Origin::Local),
      "proxy" => Ok(Origin::Proxy),
      other => match other.strip_prefix("oidc:") {
        Some(name) => Ok(Origin::Oidc(name.to_string())),
        None => Err(rusqlite::Error::InvalidColumnType(
          0,
          format!("session origin {other}"),
          rusqlite::types::Type::Text,
        )),
      },
    }
  }

  /// The provider as the API names it: `local`, `proxy` or the provider's name.
  pub fn provider(&self) -> String {
    match self {
      Origin::Local => "local".to_string(),
      Origin::Oidc(name) => name.clone(),
      Origin::Proxy => "proxy".to_string(),
    }
  }

  /// How the principal was established, as the API shows it.
  pub fn via(&self) -> Via {
    match self {
      Origin::Local => Via::Local,
      Origin::Oidc(name) => Via::Oidc {
        provider: name.clone(),
      },
      Origin::Proxy => Via::Proxy,
    }
  }
}

/// A session as stored.
#[derive(Clone)]
pub struct StoredSession {
  /// its id, which the cookie carries with its HMAC
  pub id: String,
  /// who it is for
  pub subject: String,
  /// the name to show for them
  pub name: String,
  /// its role, fixed at login
  pub role: Role,
  /// how it was established
  pub origin: Origin,
  /// the identity provider's session id
  pub idp_sid: Option<String>,
  /// the sealed ID token
  pub id_token: Option<String>,
  /// when it was made
  pub created_at: DateTime<Utc>,
  /// when it ends regardless of use
  pub expires_at: DateTime<Utc>,
  /// when it was last used, to within a tenth of the idle limit and five minutes at most
  pub last_seen_at: DateTime<Utc>,
  /// the address it was last used from
  pub ip: Option<String>,
  /// the browser it was last used from
  pub user_agent: Option<String>,
  /// when it was revoked
  pub revoked_at: Option<DateTime<Utc>>,
}

/// Leaves the id and the ID token out, so they are never logged.
impl std::fmt::Debug for StoredSession {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("StoredSession")
      .field("subject", &self.subject)
      .field("role", &self.role)
      .field("origin", &self.origin)
      .field("expires_at", &self.expires_at)
      .field("revoked_at", &self.revoked_at)
      .finish_non_exhaustive()
  }
}

impl StoredSession {
  /// Whether it may still be used at `now` given the idle limit `idle`.
  pub fn active(&self, now: DateTime<Utc>, idle: Duration) -> bool {
    self.revoked_at.is_none() && self.expires_at > now && shifted(self.last_seen_at, idle) > now
  }

  /// Whether its use at `now` is to be written, the last one written being far enough behind
  /// for the idle limit `idle`.
  pub fn seen_due(&self, now: DateTime<Utc>, idle: Duration) -> bool {
    now - self.last_seen_at >= SEEN_WRITES.min(idle / 10)
  }

  /// The principal its requests act as.
  pub fn principal(&self) -> Principal {
    Principal {
      subject: self.subject.clone(),
      name: self.name.clone(),
      role: self.role,
      via: self.origin.via(),
    }
  }

  /// The session as the sessions list shows it.
  pub fn to_api(&self, current: bool) -> Session {
    Session {
      id: self.id.clone(),
      subject: self.subject.clone(),
      name: self.name.clone(),
      via: self.origin.via(),
      role: self.role,
      created_at: self.created_at,
      last_seen_at: self.last_seen_at,
      ip: self.ip.clone(),
      user_agent: self.user_agent.clone(),
      current,
    }
  }
}

/// A configured lifetime as a time span; one too long for the span lasts for ever.
pub fn lifetime(configured: std::time::Duration) -> Duration {
  Duration::from_std(configured).unwrap_or(Duration::MAX)
}

/// The cookie value for the session `id`: the id and its HMAC.
pub fn cookie_value(secret: &Secret, id: &str) -> String {
  format!("{id}.{}", secret.sign(id))
}

/// The session id in a cookie value, when its HMAC is right.
pub fn verified_id<'a>(secret: &Secret, value: &'a str) -> Option<&'a str> {
  let (id, signature) = value.split_once('.')?;
  secret.verify(id, signature).then_some(id)
}

/// Who signed in and how, for a new session.
pub struct NewSession {
  /// the stable id of who signed in
  pub subject: String,
  /// the name to show
  pub name: String,
  /// their email address, when known
  pub email: Option<String>,
  /// their groups as JSON, for an identity provider or proxy login
  pub groups: Option<String>,
  /// the role, fixed for the session's life
  pub role: Role,
  /// how they signed in
  pub origin: Origin,
  /// how long the session lasts
  pub ttl: Duration,
  /// the identity provider's session id
  pub idp_sid: Option<String>,
  /// the sealed ID token
  pub id_token: Option<String>,
  /// where they signed in from
  pub ip: Option<String>,
  /// their browser
  pub user_agent: Option<String>,
}

const COLUMNS: &str = "s.id, s.subject, coalesce(i.name, s.subject), s.role, s.via, s.idp_sid, \
                       s.id_token, s.created_at, s.expires_at, s.last_seen_at, s.ip, \
                       s.user_agent, s.revoked_at";

const FROM: &str = "sessions s LEFT JOIN identities i ON i.subject = s.subject";

fn read_row(row: &Row<'_>) -> rusqlite::Result<StoredSession> {
  let time = |index: usize| -> rusqlite::Result<DateTime<Utc>> {
    read_timestamp(&row.get::<_, String>(index)?)
  };
  Ok(StoredSession {
    id: row.get(0)?,
    subject: row.get(1)?,
    name: row.get(2)?,
    role: row.get::<_, StoredRole>(3)?.0,
    origin: Origin::parse(&row.get::<_, String>(4)?)?,
    idp_sid: row.get(5)?,
    id_token: row.get(6)?,
    created_at: time(7)?,
    expires_at: time(8)?,
    last_seen_at: time(9)?,
    ip: row.get(10)?,
    user_agent: row.get(11)?,
    revoked_at: row
      .get::<_, Option<String>>(12)?
      .as_deref()
      .map(read_timestamp)
      .transpose()?,
  })
}

/// Records the sign-in of `new.subject` and makes a session for it.
pub async fn create(db: &Database, new: NewSession) -> Result<StoredSession, DbError> {
  let now = Utc::now();
  let session = StoredSession {
    id: URL_SAFE_NO_PAD.encode(crypto::random_bytes(ID_BYTES)),
    subject: new.subject,
    name: new.name,
    role: new.role,
    origin: new.origin,
    idp_sid: new.idp_sid,
    id_token: new.id_token,
    created_at: now,
    expires_at: shifted(now, new.ttl),
    last_seen_at: now,
    ip: new.ip,
    user_agent: new.user_agent,
    revoked_at: None,
  };
  let row = session.clone();
  let (email, groups) = (new.email, new.groups);
  db.write(move |conn| {
    let tx = conn.transaction()?;
    tx.execute(
      "INSERT INTO identities (subject, provider, name, email, claims_json, first_login_at, \
       last_login_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) ON CONFLICT (subject) DO UPDATE SET \
       provider = excluded.provider, name = excluded.name, email = coalesce(excluded.email, \
       email), claims_json = coalesce(excluded.claims_json, claims_json), last_login_at = \
       excluded.last_login_at",
      params![
        row.subject,
        row.origin.stored(),
        row.name,
        email,
        groups,
        timestamp(now)
      ],
    )?;
    tx.execute(
      "INSERT INTO sessions (id, subject, role, via, idp_sid, id_token, created_at, expires_at, \
       last_seen_at, ip, user_agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?9, ?10)",
      params![
        row.id,
        row.subject,
        StoredRole(row.role),
        row.origin.stored(),
        row.idp_sid,
        row.id_token,
        timestamp(row.created_at),
        timestamp(row.expires_at),
        row.ip,
        row.user_agent,
      ],
    )?;
    tx.commit()
  })
  .await?;
  Ok(session)
}

/// The session `id`, active or not.
pub async fn find(db: &Database, id: &str) -> Result<Option<StoredSession>, DbError> {
  let id = id.to_string();
  db.read(move |conn| {
    conn
      .query_row(
        &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.id = ?1"),
        [id],
        read_row,
      )
      .optional()
  })
  .await
}

/// Records that the session `id` was used now from `ip` with `user_agent`.
pub async fn seen(
  db: &Database,
  id: &str,
  ip: String,
  user_agent: Option<String>,
) -> Result<(), DbError> {
  let id = id.to_string();
  db.write(move |conn| {
    conn.execute(
      "UPDATE sessions SET last_seen_at = ?1, ip = ?2, user_agent = coalesce(?3, user_agent) \
       WHERE id = ?4",
      params![timestamp(Utc::now()), ip, user_agent, id],
    )
  })
  .await
  .map(drop)
}

/// The sessions active at `now` with the idle limit `idle`, of `subject` or of everyone, newest
/// first.
pub async fn active(
  db: &Database,
  subject: Option<String>,
  idle: Duration,
) -> Result<Vec<StoredSession>, DbError> {
  let now = Utc::now();
  let sessions = db
    .read(move |conn| {
      let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM {FROM} WHERE s.revoked_at IS NULL AND s.expires_at > ?1 AND (?2 \
         IS NULL OR s.subject = ?2) ORDER BY s.created_at DESC"
      ))?;
      statement
        .query_map(params![timestamp(now), subject], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
    })
    .await?;
  Ok(
    sessions
      .into_iter()
      .filter(|session| session.active(now, idle))
      .collect(),
  )
}

/// Revokes the sessions `ids` that are not revoked yet and returns those it revoked.
pub async fn revoke(db: &Database, ids: Vec<String>) -> Result<Vec<StoredSession>, DbError> {
  db.write(move |conn| {
    let now = timestamp(Utc::now());
    let tx = conn.transaction()?;
    let mut revoked = Vec::new();
    for id in ids {
      let session = tx
        .query_row(
          &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.id = ?1 AND s.revoked_at IS NULL"),
          [&id],
          read_row,
        )
        .optional()?;
      if let Some(session) = session {
        tx.execute(
          "UPDATE sessions SET revoked_at = ?1 WHERE id = ?2",
          params![now, id],
        )?;
        revoked.push(session);
      }
    }
    tx.commit()?;
    Ok(revoked)
  })
  .await
}

/// The ids of the sessions not yet revoked that came from the identity provider `origin`, by
/// the provider's session id when it gives one, else by `subject`.
pub async fn of_provider_login(
  db: &Database,
  origin: &Origin,
  idp_sid: Option<String>,
  subject: Option<String>,
) -> Result<Vec<String>, DbError> {
  let via = origin.stored();
  db.read(move |conn| {
    let mut statement = conn.prepare(
      "SELECT id FROM sessions WHERE revoked_at IS NULL AND via = ?1 AND (CASE WHEN ?2 IS NOT \
       NULL THEN idp_sid = ?2 ELSE subject = ?3 END)",
    )?;
    statement
      .query_map(params![via, idp_sid, subject], |row| row.get(0))?
      .collect::<rusqlite::Result<Vec<String>>>()
  })
  .await
}

/// The ids of the sessions that ended by time, by expiry or by going unused for `idle`, and
/// are not marked as ended yet.
pub async fn lapsed(db: &Database, idle: Duration) -> Result<Vec<String>, DbError> {
  let now = Utc::now();
  db.read(move |conn| {
    let mut statement = conn.prepare(
      "SELECT id FROM sessions WHERE revoked_at IS NULL AND (expires_at <= ?1 OR last_seen_at \
       <= ?2)",
    )?;
    statement
      .query_map(
        params![timestamp(now), timestamp(shifted(now, -idle))],
        |row| row.get(0),
      )?
      .collect::<rusqlite::Result<Vec<String>>>()
  })
  .await
}

/// Deletes the sessions that ended before `before`; how many.
pub async fn prune(db: &Database, before: DateTime<Utc>) -> Result<usize, DbError> {
  db.write(move |conn| {
    conn.execute(
      "DELETE FROM sessions WHERE revoked_at < ?1 OR expires_at < ?1",
      [timestamp(before)],
    )
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_session_in_use_outlasts_a_short_idle_limit() {
    let start: DateTime<Utc> = "2026-11-04T12:00:00Z".parse().unwrap();
    for idle in [
      Duration::seconds(30),
      Duration::minutes(5),
      Duration::hours(1),
    ] {
      let mut session = StoredSession {
        id: "id".into(),
        subject: "alice".into(),
        name: "alice".into(),
        role: Role::ReadWrite,
        origin: Origin::Local,
        idp_sid: None,
        id_token: None,
        created_at: start,
        expires_at: start + Duration::days(1),
        last_seen_at: start,
        ip: None,
        user_agent: None,
        revoked_at: None,
      };
      // a request every two seconds for two hours, each recording its use when it is due
      let mut now = start;
      while now < start + Duration::hours(2) {
        now += Duration::seconds(2);
        assert!(session.active(now, idle), "signed out at {now} with {idle}");
        if session.seen_due(now, idle) {
          session.last_seen_at = now;
        }
      }
    }
  }
}
