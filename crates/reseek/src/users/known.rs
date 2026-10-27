//! The users who have a row, each with the country last seen for them, held in memory so a
//! room's member list and every search response are matched against the directory without a
//! database read. The row writes keep it: a changed row is known, a removed one forgotten.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;

/// Every user with a row, and the country code stored for them.
#[derive(Default)]
pub struct Known(Mutex<HashMap<String, Option<String>>>);

impl Known {
  /// The users of `users.db` as `conn` reads them.
  pub fn read(conn: &Connection) -> rusqlite::Result<Self> {
    let users = conn
      .prepare("SELECT username, country_code FROM users")?
      .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
      .collect::<rusqlite::Result<_>>()?;
    Ok(Self(Mutex::new(users)))
  }

  fn users(&self) -> MutexGuard<'_, HashMap<String, Option<String>>> {
    // every change under the lock is one insert or removal
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// The country stored for `username`, when they have a row and one was seen.
  pub fn country(&self, username: &str) -> Option<String> {
    self.users().get(username).cloned().flatten()
  }

  /// `username` has a row now.
  pub fn added(&self, username: &str) {
    self.users().entry(username.to_string()).or_insert(None);
  }

  /// `username` has no row any more.
  pub fn removed(&self, username: &str) {
    self.users().remove(username);
  }

  /// Of `countries`, as `(username, code)`, those of users with a row whose stored country
  /// differs.
  pub fn news(&self, countries: Vec<(String, String)>) -> Vec<(String, String)> {
    let users = self.users();
    countries
      .into_iter()
      .filter(|(username, code)| {
        users
          .get(username)
          .is_some_and(|stored| stored.as_deref() != Some(code.as_str()))
      })
      .collect()
  }

  /// `username`'s row now holds `code`.
  pub fn set_country(&self, username: &str, code: String) {
    if let Some(stored) = self.users().get_mut(username) {
      *stored = Some(code);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_users_with_a_row_and_a_new_country_are_news() {
    let known = Known::default();
    known.added("alice");
    known.added("bob");
    known.set_country("bob", "NL".into());
    let seen = vec![
      ("alice".to_string(), "DE".to_string()),
      ("bob".into(), "NL".into()),
      ("carol".into(), "FR".into()),
    ];
    assert_eq!(known.news(seen), [("alice".to_string(), "DE".to_string())]);
    known.added("bob");
    assert_eq!(
      known.country("bob").as_deref(),
      Some("NL"),
      "kept on a change"
    );
    known.removed("bob");
    assert_eq!(known.country("bob"), None);
  }
}
