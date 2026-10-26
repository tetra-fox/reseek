//! Who is blocked from what: our blocks, kept in `blocks` and in memory for the check every
//! network edge makes, and the configuration's blacklist of names, patterns, networks and the
//! blocklist file. Each answer is remembered a while, keyed by user, address and scope.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use api_types::blocking::{BlacklistRules, Block, BlockScope, BlockSource, BlocklistFile};
use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};
use rusqlite::{Connection, OptionalExtension, Row, params};
use tracing::warn;

use super::blocklist::Blocklist;
use super::store::ensure;
use crate::config::Options;
use crate::config::cidr::IpRange;
use crate::db::{read_timestamp, timestamp};

/// How long an answer is remembered.
const DECISION_FOR: Duration = Duration::from_secs(600);
/// How many answers are remembered.
const DECISIONS: usize = 1000;
/// The size limit a blacklist pattern compiles within.
const PATTERN_SIZE: usize = 10 * 1024 * 1024;

/// Every scope in the order they are written.
pub const SCOPES: [BlockScope; 7] = [
  BlockScope::Uploads,
  BlockScope::Searches,
  BlockScope::Browse,
  BlockScope::Messages,
  BlockScope::Rooms,
  BlockScope::Downloads,
  BlockScope::Distributed,
];

/// The bit a scope is stored as.
pub fn bit(scope: BlockScope) -> u32 {
  match scope {
    BlockScope::Uploads => 1,
    BlockScope::Searches => 2,
    BlockScope::Browse => 4,
    BlockScope::Messages => 8,
    BlockScope::Rooms => 16,
    BlockScope::Downloads => 32,
    BlockScope::Distributed => 64,
  }
}

/// The bits of a list of scopes.
pub fn bits(scopes: &[BlockScope]) -> u32 {
  scopes.iter().fold(0, |all, scope| all | bit(*scope))
}

/// The scopes stored as `bits`, in their order.
pub fn scopes(bits: u32) -> Vec<BlockScope> {
  SCOPES.into_iter().filter(|s| bits & bit(*s) != 0).collect()
}

/// The configuration's block of a user it names: every scope, no time, no one.
pub fn configured(username: &str) -> Block {
  Block {
    username: username.to_string(),
    reason: String::new(),
    scopes: SCOPES.to_vec(),
    created_at: None,
    expires_at: None,
    created_by: None,
    source: BlockSource::Config,
  }
}

/// The columns [`block`] reads, in its order.
const COLUMNS: &str = "username, reason, scopes, created_at, expires_at, created_by";

fn block(row: &Row) -> rusqlite::Result<Block> {
  let expires_at = row
    .get::<_, Option<String>>(4)?
    .as_deref()
    .map(read_timestamp)
    .transpose()?;
  Ok(Block {
    username: row.get(0)?,
    reason: row.get(1)?,
    scopes: scopes(row.get(2)?),
    created_at: Some(read_timestamp(&row.get::<_, String>(3)?)?),
    expires_at,
    created_by: Some(row.get(5)?),
    source: BlockSource::Api,
  })
}

/// Every block in `blocks` that has not expired by `now`, ordered by username.
pub fn stored(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<Vec<Block>> {
  let mut statement = conn.prepare_cached(&format!(
    "SELECT {COLUMNS} FROM blocks WHERE expires_at IS NULL OR expires_at > ?1 ORDER BY username"
  ))?;
  let rows = statement.query_map([timestamp(now)], block)?;
  rows.collect()
}

/// `username`'s block in `blocks`, expired or not.
pub fn stored_of(conn: &Connection, username: &str) -> rusqlite::Result<Option<Block>> {
  conn
    .query_row(
      &format!("SELECT {COLUMNS} FROM blocks WHERE username = ?1"),
      [username],
      block,
    )
    .optional()
}

/// Stores `block`, replacing the user's earlier one, and makes the user's row when there is
/// none.
pub fn store(conn: &mut Connection, block: &Block, now: DateTime<Utc>) -> rusqlite::Result<()> {
  let tx = conn.transaction()?;
  ensure(&tx, &block.username, now)?;
  tx.execute(
    "INSERT INTO blocks (username, reason, scopes, created_at, expires_at, created_by) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
     ON CONFLICT (username) DO UPDATE SET reason = excluded.reason, scopes = excluded.scopes, \
     created_at = excluded.created_at, expires_at = excluded.expires_at, \
     created_by = excluded.created_by",
    params![
      block.username,
      block.reason,
      bits(&block.scopes),
      timestamp(block.created_at.unwrap_or(now)),
      block.expires_at.map(timestamp),
      block.created_by.clone().unwrap_or_default(),
    ],
  )?;
  tx.commit()
}

/// Deletes `username`'s block; false when there was none.
pub fn delete(conn: &Connection, username: &str) -> rusqlite::Result<bool> {
  Ok(conn.execute("DELETE FROM blocks WHERE username = ?1", [username])? == 1)
}

/// Deletes the blocks that expired by `now` and returns their users.
pub fn expire(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<Vec<String>> {
  let mut statement = conn.prepare_cached(
    "DELETE FROM blocks WHERE expires_at IS NOT NULL AND expires_at <= ?1 RETURNING username",
  )?;
  let rows = statement.query_map([timestamp(now)], |row| row.get(0))?;
  rows.collect()
}

/// The configuration's blacklist, compiled.
#[derive(Debug, Default)]
struct Rules {
  members: HashSet<String>,
  patterns: Vec<Regex>,
  cidrs: Vec<IpRange>,
  /// the texts as configured, for listing
  pattern_texts: Vec<String>,
  cidr_texts: Vec<String>,
}

impl Rules {
  fn of(options: &Options) -> Self {
    let blacklisted = &options.transfers.groups.blacklisted;
    let patterns = blacklisted
      .patterns
      .iter()
      .filter_map(|text| {
        RegexBuilder::new(text)
          .case_insensitive(!options.flags.case_sensitive_regex)
          .size_limit(PATTERN_SIZE)
          .build()
          .inspect_err(|e| warn!(pattern = text, error = %e, "blacklist pattern not used"))
          .ok()
      })
      .collect();
    let cidrs = blacklisted
      .cidrs
      .iter()
      .filter_map(|text| {
        text
          .parse::<IpRange>()
          .inspect_err(|e| warn!(cidr = text, error = %e, "blacklist network not used"))
          .ok()
      })
      .collect();
    Self {
      members: blacklisted.members.iter().cloned().collect(),
      patterns,
      cidrs,
      pattern_texts: blacklisted.patterns.clone(),
      cidr_texts: blacklisted.cidrs.clone(),
    }
  }

  fn blocks(&self, username: &str, ip: Option<IpAddr>, blocklist: Option<&Blocklist>) -> bool {
    self.members.contains(username)
      || self.patterns.iter().any(|p| p.is_match(username))
      || ip.is_some_and(|ip| {
        self.cidrs.iter().any(|range| range.contains(ip))
          || blocklist.is_some_and(|list| list.contains(ip))
      })
  }
}

/// An unexpired block of ours, as the check needs it.
#[derive(Clone, Copy, Debug)]
struct Active {
  bits: u32,
  expires_at: Option<DateTime<Utc>>,
}

/// The loaded blocklist file.
#[derive(Debug)]
struct LoadedBlocklist {
  path: PathBuf,
  list: Arc<Blocklist>,
}

type DecisionKey = (String, Option<IpAddr>, BlockScope);

/// Who is blocked: our blocks and the configuration's blacklist, with the remembered answers.
#[derive(Default)]
pub struct Blocks {
  active: RwLock<HashMap<String, Active>>,
  rules: RwLock<Arc<Rules>>,
  blocklist: RwLock<Option<LoadedBlocklist>>,
  decisions: Mutex<HashMap<DecisionKey, (bool, Instant)>>,
}

impl Blocks {
  /// The check for `options`, with our unexpired blocks `ours`.
  pub fn new(options: &Options, ours: &[Block]) -> Self {
    let blocks = Self::default();
    blocks.configure(options);
    for block in ours {
      blocks.set(block);
    }
    blocks
  }

  /// Takes the configuration's blacklist from `options` and forgets every remembered answer.
  pub fn configure(&self, options: &Options) {
    *self.rules.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(Rules::of(options));
    self
      .decisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .clear();
  }

  /// Takes the blocklist file loaded from `path`, `None` for none.
  pub fn set_blocklist(&self, loaded: Option<(PathBuf, Blocklist)>) {
    *self
      .blocklist
      .write()
      .unwrap_or_else(PoisonError::into_inner) = loaded.map(|(path, list)| LoadedBlocklist {
      path,
      list: Arc::new(list),
    });
    self
      .decisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .clear();
  }

  /// Takes our block of a user, new or replaced.
  pub fn set(&self, block: &Block) {
    self
      .active
      .write()
      .unwrap_or_else(PoisonError::into_inner)
      .insert(
        block.username.clone(),
        Active {
          bits: bits(&block.scopes),
          expires_at: block.expires_at,
        },
      );
    self.forget(&block.username);
  }

  /// Drops our block of `username`.
  pub fn unset(&self, username: &str) {
    self
      .active
      .write()
      .unwrap_or_else(PoisonError::into_inner)
      .remove(username);
    self.forget(username);
  }

  fn forget(&self, username: &str) {
    self
      .decisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .retain(|(name, _, _), _| name != username);
  }

  /// Whether the configuration blocks `username` by name or pattern.
  pub fn configuration_blocks(&self, username: &str) -> bool {
    self
      .rules
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .blocks(username, None, None)
  }

  /// The users the configuration names.
  pub fn configured_members(&self) -> Vec<String> {
    let rules = self.rules.read().unwrap_or_else(PoisonError::into_inner);
    let mut members: Vec<String> = rules.members.iter().cloned().collect();
    members.sort();
    members
  }

  /// The configuration's rules that block without naming a user.
  pub fn rules(&self) -> BlacklistRules {
    let rules = self.rules.read().unwrap_or_else(PoisonError::into_inner);
    let blocklist = self
      .blocklist
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    BlacklistRules {
      patterns: rules.pattern_texts.clone(),
      cidrs: rules.cidr_texts.clone(),
      blocklist_file: blocklist.as_ref().map(|loaded| BlocklistFile {
        path: loaded.path.display().to_string(),
        entries: u32::try_from(loaded.list.count()).unwrap_or(u32::MAX),
      }),
    }
  }

  /// Whether `username`, connecting from `ip` when known, is blocked from `scope`: by our
  /// block with that scope, or by the configuration's blacklist, which blocks from every
  /// scope. A user blocked without an address is blocked from every address.
  pub fn is_blocked(&self, username: &str, ip: Option<IpAddr>, scope: BlockScope) -> bool {
    let key = (username.to_string(), ip, scope);
    {
      let decisions = self
        .decisions
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
      let remembered = |key: &DecisionKey| {
        decisions
          .get(key)
          .filter(|(_, at)| at.elapsed() < DECISION_FOR)
          .map(|(blocked, _)| *blocked)
      };
      if let Some(blocked) = remembered(&key) {
        return blocked;
      }
      if ip.is_some() && remembered(&(username.to_string(), None, scope)) == Some(true) {
        return true;
      }
    }
    let ours = self
      .active
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .get(username)
      .is_some_and(|active| {
        active.bits & bit(scope) != 0 && active.expires_at.is_none_or(|end| end > Utc::now())
      });
    let blocked = ours || {
      let rules = self
        .rules
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
      let blocklist = self
        .blocklist
        .read()
        .unwrap_or_else(PoisonError::into_inner);
      rules.blocks(username, ip, blocklist.as_ref().map(|loaded| &*loaded.list))
    };
    let mut decisions = self
      .decisions
      .lock()
      .unwrap_or_else(PoisonError::into_inner);
    if decisions.len() >= DECISIONS {
      decisions.retain(|_, (_, at)| at.elapsed() < DECISION_FOR);
    }
    if decisions.len() >= DECISIONS
      && let Some(oldest) = decisions
        .iter()
        .min_by_key(|(_, (_, at))| *at)
        .map(|(k, _)| k.clone())
    {
      decisions.remove(&oldest);
    }
    decisions.insert(key, (blocked, Instant::now()));
    blocked
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ours(username: &str, scopes: &[BlockScope]) -> Block {
    Block {
      username: username.to_string(),
      reason: String::new(),
      scopes: scopes.to_vec(),
      created_at: Some(Utc::now()),
      expires_at: None,
      created_by: Some("admin".to_string()),
      source: BlockSource::Api,
    }
  }

  #[test]
  fn scopes_round_trip_through_their_bits() {
    let some = [
      BlockScope::Uploads,
      BlockScope::Rooms,
      BlockScope::Distributed,
    ];
    assert_eq!(bits(&some), 81);
    assert_eq!(scopes(81), some);
    assert_eq!(bits(&SCOPES[..5]), 31, "the default scopes");
  }

  #[test]
  fn a_block_change_is_seen_at_once_despite_the_remembered_answer() {
    let blocks = Blocks::new(&Options::default(), &[]);
    assert!(!blocks.is_blocked("alice", None, BlockScope::Uploads));
    blocks.set(&ours("alice", &[BlockScope::Uploads]));
    assert!(blocks.is_blocked("alice", None, BlockScope::Uploads));
    assert!(
      !blocks.is_blocked("alice", None, BlockScope::Downloads),
      "only its scopes"
    );
    assert!(
      blocks.is_blocked(
        "alice",
        Some("192.0.2.1".parse().unwrap()),
        BlockScope::Uploads
      ),
      "blocked without an address is blocked from every one"
    );
    blocks.unset("alice");
    assert!(!blocks.is_blocked("alice", None, BlockScope::Uploads));
  }

  #[test]
  fn the_configuration_blocks_by_name_pattern_and_network_from_every_scope() {
    let mut options = Options::default();
    let blacklisted = &mut options.transfers.groups.blacklisted;
    blacklisted.members = vec!["Mallory".to_string()];
    blacklisted.patterns = vec!["^spam".to_string()];
    blacklisted.cidrs = vec!["203.0.113.0/24".to_string()];
    let blocks = Blocks::new(&options, &[]);
    assert!(blocks.is_blocked("Mallory", None, BlockScope::Distributed));
    assert!(
      !blocks.is_blocked("mallory", None, BlockScope::Uploads),
      "names are exact"
    );
    assert!(
      blocks.is_blocked("SPAMBOT", None, BlockScope::Searches),
      "patterns ignore case"
    );
    assert!(!blocks.is_blocked("carol", None, BlockScope::Browse));
    let from = |ip: &str| blocks.is_blocked("carol", Some(ip.parse().unwrap()), BlockScope::Browse);
    assert!(
      from("203.0.113.7"),
      "a negative answer without an address does not stand for one with"
    );
    assert!(!from("198.51.100.7"));
    options.transfers.groups.blacklisted.members.clear();
    blocks.configure(&options);
    assert!(!blocks.is_blocked("Mallory", None, BlockScope::Distributed));
  }
}
