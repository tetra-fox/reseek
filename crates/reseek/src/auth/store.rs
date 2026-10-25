//! The authentication database, `auth.db`: who signed in, their browser sessions, the API
//! tokens, and the key session cookies are made with.

use api_types::auth::Role;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::db::{self, Schema};

/// The tables of `auth.db`.
pub static SCHEMA: Schema = db::schema!("auth", ["0001_initial"]);

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
