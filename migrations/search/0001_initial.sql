-- every search we started, with the responses it took once it finished
CREATE TABLE searches (
  id TEXT PRIMARY KEY,
  -- the terms joined by single spaces, then each exclusion with a leading "-"
  search_text TEXT NOT NULL,
  token INTEGER NOT NULL,
  -- "network", "user", "room" or "wishlist", with the usernames or the room as a JSON array
  scope_type TEXT NOT NULL DEFAULT 'network',
  scope_subjects TEXT NOT NULL DEFAULT '[]',
  -- the state flags: requested 1, in progress 2, completed 4, cancelled 8, timed out 16,
  -- response limit 32, file limit 64, errored 128, queued 256
  state INTEGER NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  response_count INTEGER NOT NULL DEFAULT 0,
  file_count INTEGER NOT NULL DEFAULT 0,
  locked_file_count INTEGER NOT NULL DEFAULT 0,
  -- the size of `responses`
  response_bytes INTEGER NOT NULL DEFAULT 0,
  -- the accepted responses as a zlib-compressed JSON array, written once the search finished;
  -- null before, and for a search whose responses were lost
  responses BLOB
);
CREATE INDEX searches_ended_at ON searches(ended_at);

-- saved searches the wishlist runs again, one a turn on the server's wishlist interval
CREATE TABLE wishlist (
  id TEXT PRIMARY KEY,
  search_text TEXT NOT NULL,
  added_at TEXT NOT NULL,
  last_searched_at TEXT,
  last_search_id TEXT,
  last_new_files INTEGER NOT NULL DEFAULT 0
);

-- the files each wishlist item found, by user and remote filename, so a later turn tells only
-- of new ones
CREATE TABLE wishlist_seen (
  item_id TEXT NOT NULL,
  username TEXT NOT NULL,
  filename TEXT NOT NULL,
  PRIMARY KEY (item_id, username, filename)
) WITHOUT ROWID;
