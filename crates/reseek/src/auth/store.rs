//! The authentication database, `auth.db`: who signed in, their browser sessions, the API
//! tokens, and the key session cookies are made with.

use api_types::auth::Role;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::db::{self, Schema};

/// The tables of `auth.db`.
pub static SCHEMA: Schema = db::schema!("auth", ["0001_initial", "0002_session_role"], private);

/// A role as the database stores it: 0, 1 and 3, as the roles have always been numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredRole(pub Role);

impl ToSql for StoredRole {
  fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
    let number: i64 = match self.0 {
      Role::ReadOnly => 0,
      Role::ReadWrite => 1,
      Role::Administrator => 3,
    };
    Ok(number.into())
  }
}

impl FromSql for StoredRole {
  fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
    match value.as_i64()? {
      0 => Ok(Self(Role::ReadOnly)),
      1 => Ok(Self(Role::ReadWrite)),
      3 => Ok(Self(Role::Administrator)),
      other => Err(FromSqlError::OutOfRange(other)),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{Database, Location};

  /// `auth.db` while sessions kept their role.
  static BEFORE_SESSION_ROLES: Schema = db::schema!("auth", ["0001_initial"], private);

  #[tokio::test]
  async fn session_roles_go_and_identity_provider_emails_are_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let old = Database::open(&BEFORE_SESSION_ROLES, &location)
      .await
      .unwrap();
    old
      .write(|conn| {
        for (subject, provider, email) in [
          ("oidc:home:u1", "oidc:home", "unverified@example.net"),
          ("proxy:alice", "proxy", "alice@example.net"),
        ] {
          conn.execute(
            "INSERT INTO identities (subject, provider, email, first_login_at, last_login_at) \
             VALUES (?1, ?2, ?3, '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z')",
            [subject, provider, email],
          )?;
        }
        conn.execute(
          "INSERT INTO sessions (id, subject, role, via, created_at, expires_at, last_seen_at) \
           VALUES ('s1', 'oidc:home:u1', 3, 'oidc:home', '2026-11-01T00:00:00Z', \
           '2026-11-02T00:00:00Z', '2026-11-01T00:00:00Z')",
          [],
        )
      })
      .await
      .unwrap();
    drop(old);
    let migrated = Database::open(&SCHEMA, &location).await.unwrap();
    let (emails, sessions) = migrated
      .read(|conn| {
        let mut statement =
          conn.prepare("SELECT subject, email FROM identities ORDER BY subject")?;
        let emails = statement
          .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
          .collect::<rusqlite::Result<Vec<(String, Option<String>)>>>()?;
        let sessions: i64 =
          conn.query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))?;
        Ok((emails, sessions))
      })
      .await
      .unwrap();
    assert_eq!(
      emails,
      [
        ("oidc:home:u1".to_string(), None),
        (
          "proxy:alice".to_string(),
          Some("alice@example.net".to_string())
        )
      ]
    );
    assert_eq!(sessions, 1, "the session stays");
  }
}
