-- every user we dealt with: when, what the server and the peer last said, and our own notes
CREATE TABLE users (
  -- exact and case-sensitive, as the server treats it
  username TEXT PRIMARY KEY,
  first_seen_at TEXT NOT NULL,
  -- the last interaction of any kind
  last_seen_at TEXT NOT NULL,
  -- "Online", "Away" or "Offline", null when never observed
  last_presence TEXT,
  presence_at TEXT,
  is_privileged INTEGER NOT NULL DEFAULT 0,
  -- the statistics the server last gave, with when they were fetched
  average_speed INTEGER,
  upload_count INTEGER,
  file_count INTEGER,
  directory_count INTEGER,
  statistics_at TEXT,
  -- from a room member list, if ever seen
  country_code TEXT,
  -- what the peer last said about itself, with when it was fetched
  description TEXT,
  has_picture INTEGER,
  picture BLOB,
  upload_slots INTEGER,
  queue_length INTEGER,
  has_free_slot INTEGER,
  info_at TEXT,
  -- our private free text
  notes TEXT NOT NULL DEFAULT '',
  -- a JSON array of lower-case, unique labels
  tags TEXT NOT NULL DEFAULT '[]',
  -- a user-defined upload group, null for none
  group_override TEXT,
  -- chat notifications off for this user
  muted INTEGER NOT NULL DEFAULT 0,
  -- listed first
  pinned INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX users_last_seen ON users(last_seen_at DESC);
CREATE INDEX users_pinned ON users(pinned) WHERE pinned = 1;

-- our blocks of users; the configuration's blocks are not stored
CREATE TABLE blocks (
  username TEXT PRIMARY KEY REFERENCES users(username) ON DELETE CASCADE,
  reason TEXT NOT NULL DEFAULT '',
  -- what the block stops, as bits: uploads 1, searches 2, browse 4, messages 8, rooms 16,
  -- downloads 32, distributed 64
  scopes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  -- null for a block that does not end
  expires_at TEXT,
  -- the name of the principal that made it
  created_by TEXT NOT NULL
);
CREATE INDEX blocks_expiry ON blocks(expires_at) WHERE expires_at IS NOT NULL;
