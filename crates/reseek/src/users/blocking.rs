//! Who is blocked from what: our blocks, kept in `blocks` and in memory for the check every
//! network edge makes, and the configuration's blacklist of names, patterns, networks and the
//! blocklist file. A block of ours may name addresses too, and then blocks anyone connecting
//! from them from its scopes. Each answer is remembered a while, keyed by user, address and
//! scope.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use api_types::users::blocking::{BlacklistRules, Block, BlockScope, BlockSource, BlocklistFile};
use chrono::{DateTime, Utc};
use regex::Regex;
use reseek_shares::lock;
use rusqlite::{Connection, OptionalExtension, Row, params};
use tracing::warn;

use super::blocklist::Blocklist;
use super::group_cache::Reported;
use super::store::ensure;
use crate::config::Options;
use crate::config::cidr::IpRange;
use crate::db::{read_timestamp, timestamp};

/// How long an answer is remembered.
const DECISION_FOR: Duration = Duration::from_secs(600);
/// How many answers are remembered.
const DECISIONS: usize = 1000;

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
  BlockScope::ALL
    .into_iter()
    .filter(|s| bits & bit(*s) != 0)
    .collect()
}

/// The configuration's block of a user it names: every scope, no time, no one.
pub fn configured(username: &str) -> Block {
  Block {
    username: username.to_string(),
    reason: String::new(),
    scopes: BlockScope::ALL.to_vec(),
    addresses: Vec::new(),
    created_at: None,
    expires_at: None,
    created_by: None,
    source: BlockSource::Config,
  }
}

/// The most characters a block's reason has.
const REASON_CHARS: usize = 500;

/// Whether `reason` is short enough to be a block's reason.
pub fn reason_fits(reason: &str) -> bool {
  reason.chars().count() <= REASON_CHARS
}

/// The most addresses a block names.
pub const ADDRESSES: usize = 16;

/// `addresses` trimmed and without repeats, each an IP address, a network or a range; the
/// error says there are too many, or names the first that is none of them.
pub fn addresses_of(addresses: Vec<String>) -> Result<Vec<String>, String> {
  if addresses.len() > ADDRESSES {
    return Err(format!("A block names at most {ADDRESSES} addresses"));
  }
  let mut kept: Vec<String> = Vec::with_capacity(addresses.len());
  for address in addresses {
    let address = address.trim().to_string();
    if let Err(e) = address.parse::<IpRange>() {
      return Err(format!("Address '{address}' is invalid: {e}"));
    }
    if !kept.contains(&address) {
      kept.push(address);
    }
  }
  Ok(kept)
}

/// The columns [`block`] reads, in its order.
const COLUMNS: &str = "username, reason, scopes, created_at, expires_at, created_by, addresses";

fn block(row: &Row) -> rusqlite::Result<Block> {
  let expires_at = row
    .get::<_, Option<String>>(4)?
    .as_deref()
    .map(read_timestamp)
    .transpose()?;
  let addresses: String = row.get(6)?;
  let addresses = serde_json::from_str(&addresses).map_err(|e| {
    rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
  })?;
  Ok(Block {
    username: row.get(0)?,
    reason: row.get(1)?,
    scopes: scopes(row.get(2)?),
    addresses,
    created_at: Some(read_timestamp(&row.get::<_, String>(3)?)?),
    expires_at,
    created_by: Some(row.get(5)?),
    source: BlockSource::Api,
  })
}

/// Whether a block that ends at `expires_at`, never when `None`, still stands at `now`.
pub fn stands(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
  expires_at.is_none_or(|end| end > now)
}

/// [`stands`] as SQL, for the `blocks` row `row` and the time bound as `now`, such as `?1`.
pub fn stands_sql(row: &str, now: &str) -> String {
  format!("({row}.expires_at IS NULL OR {row}.expires_at > {now})")
}

/// Every block in `blocks` that still stands at `now`, ordered by username.
pub fn stored(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<Vec<Block>> {
  let mut statement = conn.prepare_cached(&format!(
    "SELECT {COLUMNS} FROM blocks WHERE {} ORDER BY username",
    stands_sql("blocks", "?1")
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
pub fn store(
  conn: &mut Connection,
  block: &Block,
  now: DateTime<Utc>,
  reported: &Reported,
) -> rusqlite::Result<()> {
  // only a block from the configuration has no maker, and those are never stored
  let created_by = block.created_by.as_deref().ok_or_else(|| {
    rusqlite::Error::ToSqlConversionFailure("a stored block names who made it".into())
  })?;
  let addresses = serde_json::to_string(&block.addresses)
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
  let tx = conn.transaction()?;
  ensure(&tx, &block.username, now, reported)?;
  tx.execute(
    "INSERT INTO blocks (username, reason, scopes, created_at, expires_at, created_by, addresses) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
     ON CONFLICT (username) DO UPDATE SET reason = excluded.reason, scopes = excluded.scopes, \
     created_at = excluded.created_at, expires_at = excluded.expires_at, \
     created_by = excluded.created_by, addresses = excluded.addresses",
    params![
      block.username,
      block.reason,
      bits(&block.scopes),
      timestamp(block.created_at.unwrap_or(now)),
      block.expires_at.map(timestamp),
      created_by,
      addresses,
    ],
  )?;
  tx.commit()
}

/// Deletes `username`'s block if it still stands at `now`; false when there was none. An
/// expired block is left for the sweep, which reports it as expired.
pub fn delete(conn: &Connection, username: &str, now: DateTime<Utc>) -> rusqlite::Result<bool> {
  let deleted = conn.execute(
    &format!(
      "DELETE FROM blocks WHERE username = ?1 AND {}",
      stands_sql("blocks", "?2")
    ),
    rusqlite::params![username, timestamp(now)],
  )?;
  Ok(deleted == 1)
}

/// Deletes the blocks that no longer stand at `now` and returns their users.
pub fn expire(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<Vec<String>> {
  let mut statement = conn.prepare_cached(&format!(
    "DELETE FROM blocks WHERE NOT {} RETURNING username",
    stands_sql("blocks", "?1")
  ))?;
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
  /// The blacklist of `options`, its name patterns matching case when `case_sensitive`.
  fn of(options: &Options, case_sensitive: bool) -> Self {
    let blacklisted = &options.transfers.groups.blacklisted;
    let patterns = blacklisted
      .patterns
      .iter()
      .filter_map(|text| {
        reseek_shares::patterns::compile(text, case_sensitive)
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
#[derive(Clone, Debug)]
struct Active {
  bits: u32,
  expires_at: Option<DateTime<Utc>>,
  /// the addresses blocked with the user
  ranges: Vec<IpRange>,
}

impl Active {
  /// Whether it blocks from `scope` at `now`.
  fn blocks(&self, scope: BlockScope, now: DateTime<Utc>) -> bool {
    self.bits & bit(scope) != 0 && stands(self.expires_at, now)
  }
}

/// The loaded blocklist file.
#[derive(Debug)]
struct LoadedBlocklist {
  path: PathBuf,
  list: Arc<Blocklist>,
}

type DecisionKey = (String, Option<IpAddr>, BlockScope);

/// A remembered answer: what it was, when it was worked out, and when it was last given.
#[derive(Clone, Copy, Debug)]
struct Decision {
  blocked: bool,
  decided: Instant,
  used: Instant,
}

impl Decision {
  fn fresh(&self) -> bool {
    self.decided.elapsed() < DECISION_FOR
  }
}

/// The remembered answers, and how many times answers were forgotten, so an answer worked out
/// while a change forgot the answers is not remembered after it.
#[derive(Default)]
struct Remembered {
  answers: HashMap<DecisionKey, Decision>,
  forgets: u64,
}

impl Remembered {
  /// Forgets the answers `forgotten` picks.
  fn forget(&mut self, forgotten: impl Fn(&DecisionKey) -> bool) {
    self.forgets += 1;
    self.answers.retain(|key, _| !forgotten(key));
  }
}

/// Who is blocked: our blocks and the configuration's blacklist, with the remembered answers.
#[derive(Default)]
pub struct Blocks {
  /// whether the blacklist's name patterns match case, as the daemon started with
  case_sensitive: bool,
  active: RwLock<HashMap<String, Active>>,
  rules: RwLock<Arc<Rules>>,
  blocklist: RwLock<Option<LoadedBlocklist>>,
  decisions: Mutex<Remembered>,
}

impl Blocks {
  /// The check for `options`, with our unexpired blocks `ours`.
  pub fn new(options: &Options, ours: &[Block]) -> Self {
    let blocks = Self {
      case_sensitive: options.flags.case_sensitive_regex,
      ..Self::default()
    };
    blocks.configure(options);
    for block in ours {
      blocks.set(block);
    }
    blocks
  }

  /// Takes the configuration's blacklist from `options` and forgets every remembered answer.
  pub fn configure(&self, options: &Options) {
    let rules = Rules::of(options, self.case_sensitive);
    *self.rules.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(rules);
    lock(&self.decisions).forget(|_| true);
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
    lock(&self.decisions).forget(|_| true);
  }

  /// Takes our block of a user, new or replaced.
  pub fn set(&self, block: &Block) {
    let ranges: Vec<IpRange> = block
      .addresses
      .iter()
      .filter_map(|text| {
        text
          .parse()
          .inspect_err(|e| warn!(address = text, error = %e, "blocked address not used"))
          .ok()
      })
      .collect();
    let addressed = !ranges.is_empty();
    let before = self
      .active
      .write()
      .unwrap_or_else(PoisonError::into_inner)
      .insert(
        block.username.clone(),
        Active {
          bits: bits(&block.scopes),
          expires_at: block.expires_at,
          ranges,
        },
      );
    self.forget_for(&block.username, before, addressed);
  }

  /// Drops our block of `username`.
  pub fn unset(&self, username: &str) {
    let before = self
      .active
      .write()
      .unwrap_or_else(PoisonError::into_inner)
      .remove(username);
    self.forget_for(username, before, false);
  }

  /// Forgets the answers a change of `username`'s block from `before` may have changed: every
  /// answer when the block named addresses before or does now (`addressed`), since those
  /// answer for other users too, else the user's own.
  fn forget_for(&self, username: &str, before: Option<Active>, addressed: bool) {
    if addressed || before.is_some_and(|b| !b.ranges.is_empty()) {
      lock(&self.decisions).forget(|_| true);
    } else {
      self.forget(username);
    }
  }

  /// Drops the remembered answers older than they are kept for.
  pub fn forget_stale(&self) {
    lock(&self.decisions)
      .answers
      .retain(|_, decision| decision.fresh());
  }

  fn forget(&self, username: &str) {
    lock(&self.decisions).forget(|(name, _, _)| name == username);
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

  /// Whether a remembered answer says `username` is blocked from `scope`, at whatever address
  /// it was worked out for; nothing is worked out, so a user without one is taken as not
  /// blocked. For a check that must stay cheap and is followed by the full one.
  pub fn remembers_blocked(&self, username: &str, scope: BlockScope) -> bool {
    lock(&self.decisions)
      .answers
      .iter()
      .any(|((name, _, remembered), decision)| {
        name == username && *remembered == scope && decision.blocked && decision.fresh()
      })
  }

  /// Whether a block of ours that has not expired blocks `username` from `scope`; the
  /// configuration's blacklist is not asked.
  pub fn ours_blocks(&self, username: &str, scope: BlockScope) -> bool {
    self
      .active
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .get(username)
      .is_some_and(|active| active.blocks(scope, Utc::now()))
  }

  /// Whether `username`, connecting from `ip` when known, is blocked from `scope`: by our
  /// block with that scope, by a block of ours with that scope naming `ip`, or by the
  /// configuration's blacklist, which blocks from every scope. A user blocked without an
  /// address is blocked from every address.
  pub fn is_blocked(&self, username: &str, ip: Option<IpAddr>, scope: BlockScope) -> bool {
    let key = (username.to_string(), ip, scope);
    let forgets = {
      let mut decisions = lock(&self.decisions);
      let forgets = decisions.forgets;
      let mut remembered = |key: &DecisionKey| {
        let decision = decisions.answers.get_mut(key).filter(|d| d.fresh())?;
        decision.used = Instant::now();
        Some(decision.blocked)
      };
      if let Some(blocked) = remembered(&key) {
        return blocked;
      }
      if ip.is_some() && remembered(&(username.to_string(), None, scope)) == Some(true) {
        return true;
      }
      forgets
    };
    let now = Utc::now();
    let ours = {
      let active = self.active.read().unwrap_or_else(PoisonError::into_inner);
      active.get(username).is_some_and(|a| a.blocks(scope, now))
        || ip.is_some_and(|ip| {
          active
            .values()
            .any(|a| a.blocks(scope, now) && a.ranges.iter().any(|range| range.contains(ip)))
        })
    };
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
    let mut remembered = lock(&self.decisions);
    // a block changed while this was worked out, so the answer may be the one from before
    if remembered.forgets != forgets {
      return blocked;
    }
    let decisions = &mut remembered.answers;
    if decisions.len() >= DECISIONS {
      decisions.retain(|_, decision| decision.fresh());
    }
    if decisions.len() >= DECISIONS
      && let Some(least) = decisions
        .iter()
        .min_by_key(|(_, decision)| decision.used)
        .map(|(k, _)| k.clone())
    {
      decisions.remove(&least);
    }
    let now = Instant::now();
    decisions.insert(
      key,
      Decision {
        blocked,
        decided: now,
        used: now,
      },
    );
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
      addresses: Vec::new(),
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
    assert_eq!(bits(&BlockScope::DEFAULT), 31, "the default scopes");
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
  fn an_address_blocked_with_a_user_blocks_anyone_from_it_in_its_scopes() {
    let blocks = Blocks::new(&Options::default(), &[]);
    let at = |ip: &str, scope| blocks.is_blocked("carol", Some(ip.parse().unwrap()), scope);
    assert!(
      !at("203.0.113.7", BlockScope::Uploads),
      "remembered before the block"
    );
    let mut alice = ours("alice", &[BlockScope::Uploads]);
    alice.addresses = vec!["203.0.113.0/24".to_string()];
    blocks.set(&alice);
    assert!(
      at("203.0.113.7", BlockScope::Uploads),
      "another user's remembered answer is worked out again"
    );
    assert!(!at("203.0.113.7", BlockScope::Messages), "only its scopes");
    assert!(!at("198.51.100.7", BlockScope::Uploads));
    assert!(!blocks.is_blocked("carol", None, BlockScope::Uploads));
    blocks.set(&ours("alice", &[BlockScope::Uploads]));
    assert!(
      !at("203.0.113.7", BlockScope::Uploads),
      "taking the address off lifts it at once"
    );
  }

  #[test]
  fn addresses_are_trimmed_kept_once_and_checked() {
    let kept = addresses_of(vec![
      " 203.0.113.7 ".to_string(),
      "203.0.113.7".to_string(),
      "2001:db8::/32".to_string(),
    ]);
    assert_eq!(kept.unwrap(), ["203.0.113.7", "2001:db8::/32"]);
    let refused = addresses_of(vec!["alice".to_string()]).unwrap_err();
    assert!(
      refused.starts_with("Address 'alice' is invalid"),
      "{refused}"
    );
    let many = (0..=ADDRESSES).map(|n| format!("192.0.2.{n}")).collect();
    assert!(addresses_of(many).is_err());
  }

  #[test]
  fn a_full_cache_drops_the_least_recently_used_answer() {
    let blocks = Blocks::new(&Options::default(), &[]);
    for n in 0..DECISIONS {
      blocks.is_blocked(&format!("user{n}"), None, BlockScope::Uploads);
    }
    blocks.is_blocked("user0", None, BlockScope::Uploads);
    blocks.is_blocked("newcomer", None, BlockScope::Uploads);
    let remembered = blocks.decisions.lock().unwrap();
    let decisions = &remembered.answers;
    let held = |name: &str| decisions.contains_key(&(name.to_string(), None, BlockScope::Uploads));
    assert_eq!(decisions.len(), DECISIONS);
    assert!(held("user0"), "asked again, so kept");
    assert!(!held("user1"), "the least recently asked");
    assert!(held("newcomer"));
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
    options.flags.case_sensitive_regex = true;
    blocks.configure(&options);
    assert!(
      blocks.is_blocked("SPAMBOT", None, BlockScope::Searches),
      "patterns match case only after a restart"
    );
  }

  #[test]
  fn the_cheap_check_only_reads_what_is_remembered() {
    let mut options = Options::default();
    options.transfers.groups.blacklisted.members = vec!["mallory".to_string()];
    options.transfers.groups.blacklisted.cidrs = vec!["203.0.113.0/24".to_string()];
    let blocks = Blocks::new(&options, &[]);
    assert!(
      !blocks.remembers_blocked("mallory", BlockScope::Searches),
      "nothing remembered yet, so not blocked"
    );
    assert!(blocks.is_blocked("mallory", None, BlockScope::Searches));
    assert!(blocks.remembers_blocked("mallory", BlockScope::Searches));
    assert!(!blocks.remembers_blocked("mallory", BlockScope::Browse));
    let ip = Some("203.0.113.7".parse().unwrap());
    assert!(blocks.is_blocked("carol", ip, BlockScope::Searches));
    assert!(
      blocks.remembers_blocked("carol", BlockScope::Searches),
      "an answer worked out with an address stands for the name"
    );
    blocks.configure(&options);
    assert!(!blocks.remembers_blocked("carol", BlockScope::Searches));
  }
}
