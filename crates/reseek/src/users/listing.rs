//! A page of the directory: the users matching every filter, buddies first within the chosen
//! order; and the list of groups with how many users are in each.

use api_types::transfers::SortOrder;
use api_types::users::{
  GroupKind, GroupList, GroupSummary, UserGroup, UserList, UserListQuery, UserSort,
};
use chrono::Utc;
use rusqlite::params_from_iter;
use rusqlite::types::Value;
use soulseek_proto::SERVER_USERNAME;

use super::store::{COLUMN_COUNT, COLUMNS, Stored, presence_text, stored};
use super::{Directory, blocking, groups, transfers};
use crate::api::error::ApiError;
use crate::config::{BUILTIN_GROUPS, DEFAULT_GROUP, PRIVILEGED_GROUP};
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
  pub(crate) async fn list(&self, query: UserListQuery) -> Result<UserList, ApiError> {
    // a row under the name the server's notices come from is no user
    let mut filter = vec!["username <> ?2".to_string()];
    let mut values = vec![
      Value::Text(timestamp(Utc::now())),
      Value::Text(SERVER_USERNAME.to_string()),
    ];
    if let Some(buddy) = query.buddy {
      filter.push(format!("buddy = {}", i32::from(buddy)));
    }
    if let Some(presence) = query.presence {
      values.push(Value::Text(presence_text(presence).to_string()));
      filter.push(format!("last_presence = ?{}", values.len()));
    }
    let sql = format!(
      "SELECT {COLUMNS}, EXISTS (SELECT 1 FROM blocks b WHERE b.username = users.username \
       AND {}) FROM users WHERE {}",
      blocking::stands_sql("b", "?1"),
      filter.join(" AND ")
    );
    let rows = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(&values), |row| {
          Ok((stored(row)?, row.get::<_, bool>(COLUMN_COUNT)?))
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
            || c.stored.groups.iter().any(|group| group.contains(needle))
        })
      })
      .filter(|c| query.blocked.is_none_or(|wanted| c.blocked == wanted))
      .filter(|c| {
        query
          .has_transfers
          .is_none_or(|wanted| c.has_transfers == wanted)
      })
      .filter(|c| {
        query
          .group
          .as_ref()
          .is_none_or(|wanted| self.placement_shown(&options, &c.stored).is_in(wanted))
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
        .buddy
        .is_some()
        .cmp(&a.stored.buddy.is_some())
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

impl Directory {
  /// Every group: privileged, the others in the order they go, then the names of groups the
  /// configuration no longer has that still have members, each with how many users the
  /// directory knows in it and the shares only some groups see that its members see.
  pub(crate) async fn group_list(&self) -> Result<GroupList, ApiError> {
    let sql = format!("SELECT {COLUMNS} FROM users WHERE username <> ?1");
    let rows = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map([SERVER_USERNAME], stored)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
      })
      .await
      .map_err(ApiError::internal)?;
    let options = self.config.current().values.clone();
    let placements: Vec<groups::Placement> = rows
      .iter()
      .map(|row| self.placement_shown(&options, row))
      .collect();
    let mut names: Vec<String> = vec![PRIVILEGED_GROUP.to_string()];
    names.extend(
      options
        .transfers
        .groups
        .ordered()
        .into_iter()
        .chain([DEFAULT_GROUP])
        .map(str::to_string),
    );
    let mut gone: Vec<&String> = rows
      .iter()
      .flat_map(|row| &row.groups)
      .filter(|name| {
        BUILTIN_GROUPS.contains(&name.as_str())
          || !options.transfers.groups.user_defined.contains_key(*name)
      })
      .collect();
    gone.sort();
    gone.dedup();
    let list: Vec<GroupSummary> = names
      .iter()
      .map(|name| (name.as_str(), groups::describe(&options, name)))
      .chain(gone.into_iter().map(|name| {
        let group = UserGroup {
          name: name.clone(),
          kind: GroupKind::Manual,
          defined: false,
          upload_settings: false,
          color: None,
        };
        (name.as_str(), group)
      }))
      .map(|(name, group)| {
        let builtin = group.defined && BUILTIN_GROUPS.contains(&name);
        let members = placements
          .iter()
          .filter(|placed| {
            if group.defined {
              placed.is_in(name)
            } else {
              placed.groups.iter().any(|g| g.name == name && !g.defined)
            }
          })
          .count() as u64;
        let mut shares: Vec<String> = options
          .shares
          .visible_to
          .iter()
          .filter(|(_, seen_by)| group.defined && seen_by.iter().any(|g| g == name))
          .map(|(alias, _)| alias.clone())
          .collect();
        shares.sort();
        GroupSummary {
          group,
          builtin,
          members,
          shares,
        }
      })
      .collect();
    Ok(GroupList { groups: list })
  }
}
