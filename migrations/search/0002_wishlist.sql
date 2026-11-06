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
