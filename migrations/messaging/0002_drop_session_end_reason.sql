-- a history gap reads the same however the session before it ended, so end_reason goes. the
-- table is rebuilt because DROP COLUMN fails on it: for a last column sqlite 3.51 cuts the stored
-- CREATE TABLE back to the nearest comma, here inside the comment above end_reason
CREATE TABLE sessions_rebuilt (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  logged_in_at TEXT NOT NULL,
  -- moved on every minute while logged in
  last_seen_at TEXT NOT NULL,
  -- null while the session runs
  ended_at TEXT
);

INSERT INTO sessions_rebuilt (id, logged_in_at, last_seen_at, ended_at)
  SELECT id, logged_in_at, last_seen_at, ended_at FROM sessions;

DROP TABLE sessions;

ALTER TABLE sessions_rebuilt RENAME TO sessions;
