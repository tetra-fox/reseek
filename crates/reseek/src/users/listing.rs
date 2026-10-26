//! A page of the directory: the users matching every filter, pinned ones first within the
//! chosen order.

use api_types::transfers::SortOrder;
use api_types::users::{UserList, UserListQuery, UserSort};
use chrono::Utc;
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::store::{COLUMNS, Stored, presence_text, stored};
use super::{Directory, groups, transfers};
use crate::api::error::ApiError;
use crate::db::timestamp;

/// A candidate row with what the filters and the sort need.
struct Candidate {
  stored: Stored,
  blocked: bool,
  downloads: u32,
  uploads: u32,
  has_transfers: bool,
}

impl Directory {
  /// The page `query` asks for.
  pub(super) async fn list(&self, query: UserListQuery) -> Result<UserList, ApiError> {
    let mut filter = vec!["1 = 1".to_string()];
    let mut values = vec![Value::Text(timestamp(Utc::now()))];
    if let Some(pinned) = query.pinned {
      filter.push(format!("pinned = {}", i32::from(pinned)));
    }
    if let Some(presence) = query.presence {
      values.push(Value::Text(presence_text(presence).to_string()));
      filter.push(format!("last_presence = ?{}", values.len()));
    }
    if let Some(tag) = &query.tag {
      values.push(Value::Text(tag.to_lowercase()));
      filter.push(format!(
        "EXISTS (SELECT 1 FROM json_each(users.tags) WHERE value = ?{})",
        values.len()
      ));
    }
    let sql = format!(
      "SELECT {COLUMNS}, EXISTS (SELECT 1 FROM blocks b WHERE b.username = users.username \
       AND (b.expires_at IS NULL OR b.expires_at > ?1)) FROM users WHERE {}",
      filter.join(" AND ")
    );
    let rows = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(&values), |row| {
          Ok((stored(row)?, row.get::<_, bool>(23)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
      })
      .await
      .map_err(ApiError::internal)?;
    let counts = transfers::succeeded_by_user(&self.transfers)
      .await
      .map_err(ApiError::internal)?;
    let options = self.config.current().values.clone();
    let needle = query.q.as_ref().map(|q| q.to_lowercase());
    let mut candidates: Vec<Candidate> = rows
      .into_iter()
      .map(|(stored, ours)| {
        let blocked = ours || self.blocks.configuration_blocks(&stored.username);
        let succeeded = counts.get(&stored.username);
        Candidate {
          blocked,
          downloads: succeeded.map_or(0, |c| c.0),
          uploads: succeeded.map_or(0, |c| c.1),
          has_transfers: succeeded.is_some(),
          stored,
        }
      })
      .filter(|c| {
        needle.as_ref().is_none_or(|needle| {
          c.stored.username.to_lowercase().contains(needle)
            || c.stored.notes.to_lowercase().contains(needle)
            || c.stored.tags.iter().any(|tag| tag.contains(needle))
        })
      })
      .filter(|c| query.blocked.is_none_or(|wanted| c.blocked == wanted))
      .filter(|c| {
        query
          .has_transfers
          .is_none_or(|wanted| c.has_transfers == wanted)
      })
      .filter(|c| {
        query.group.as_ref().is_none_or(|wanted| {
          groups::effective(&options, &c.stored.username, Some(&c.stored)).name == *wanted
        })
      })
      .collect();
    let order = query.order.unwrap_or(match query.sort {
      UserSort::Username => SortOrder::Asc,
      _ => SortOrder::Desc,
    });
    candidates.sort_by(|a, b| {
      let by = match query.sort {
        UserSort::LastSeen => a.stored.last_seen_at.cmp(&b.stored.last_seen_at),
        UserSort::FirstSeen => a.stored.first_seen_at.cmp(&b.stored.first_seen_at),
        UserSort::Username => a.stored.username.cmp(&b.stored.username),
        UserSort::Uploads => a.uploads.cmp(&b.uploads),
        UserSort::Downloads => a.downloads.cmp(&b.downloads),
      };
      let by = match order {
        SortOrder::Asc => by,
        SortOrder::Desc => by.reverse(),
      };
      b.stored
        .pinned
        .cmp(&a.stored.pinned)
        .then(by)
        .then_with(|| a.stored.username.cmp(&b.stored.username))
    });
    let total = candidates.len() as u64;
    let mut users = Vec::new();
    for candidate in candidates
      .into_iter()
      .skip(query.offset as usize)
      .take(query.limit as usize)
    {
      users.push(self.summary_of(&candidate.stored.username).await?);
    }
    Ok(UserList { users, total })
  }
}
