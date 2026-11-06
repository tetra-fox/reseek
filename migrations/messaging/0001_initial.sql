-- one private conversation per peer; a message either way opens it again once it was closed
CREATE TABLE conversations (
  -- exact and case-sensitive, as the server treats it
  username TEXT PRIMARY KEY,
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- what a conversation notifies of, and the text the UI kept unsent
CREATE TABLE conversation_state (
  username TEXT PRIMARY KEY,
  -- "all", "mentions" or "none"
  notify TEXT NOT NULL DEFAULT 'all',
  draft TEXT NOT NULL DEFAULT ''
);

-- every private message, both ways
CREATE TABLE private_messages (
  local_id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL,
  -- the server's id of an inbound message, null for an outbound one
  remote_id INTEGER,
  -- inbound: the server's time, to the second; outbound: when we sent it
  timestamp TEXT NOT NULL,
  -- 0 out, 1 in
  direction INTEGER NOT NULL,
  message TEXT NOT NULL,
  -- inbound: the server was told we read it; outbound: always 1
  is_acknowledged INTEGER NOT NULL DEFAULT 0,
  -- the server delivered it again at a login because it was not acknowledged
  was_replayed INTEGER NOT NULL DEFAULT 0,
  -- 1 for a message we sent on our own, as the answer to the first message from someone while
  -- we were away
  auto_reply INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX pm_remote ON private_messages(username, remote_id) WHERE remote_id IS NOT NULL;
CREATE INDEX pm_user_ts ON private_messages(username, timestamp);
-- a conversation's messages in the order they were stored, so its newest one is a single seek
CREATE INDEX pm_user_local ON private_messages(username, local_id);
CREATE INDEX pm_unacked ON private_messages(username) WHERE is_acknowledged = 0;

-- every message said in a room while we were in it
CREATE TABLE room_messages (
  local_id INTEGER PRIMARY KEY AUTOINCREMENT,
  room_name TEXT NOT NULL,
  -- when it arrived; the server sends no time
  timestamp TEXT NOT NULL,
  username TEXT NOT NULL,
  message TEXT NOT NULL,
  is_self INTEGER NOT NULL DEFAULT 0,
  -- it mentions our username, as it was when the message arrived
  is_mention INTEGER NOT NULL DEFAULT 0,
  -- from a user blocked for rooms, shown collapsed
  hidden INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX rm_room_ts ON room_messages(room_name, local_id);
CREATE INDEX rm_user ON room_messages(username);
CREATE INDEX rm_mentions ON room_messages(room_name, local_id) WHERE is_mention = 1;
CREATE INDEX rm_timestamp ON room_messages(timestamp);

-- what each room we were ever in keeps besides its messages
CREATE TABLE room_state (
  room_name TEXT PRIMARY KEY,
  -- the local id of the newest message the UI showed as read
  last_read_id INTEGER NOT NULL DEFAULT 0,
  -- "all", "mentions" or "none"
  notify TEXT NOT NULL DEFAULT 'mentions',
  joined_at TEXT,
  left_at TEXT,
  -- 1 for a room joined again after a start: a join asked for through the UI or the API marks
  -- the room, a leave unmarks it; a room joined only because the configuration lists it is not
  -- marked
  remembered INTEGER NOT NULL DEFAULT 0
);

-- each time we were logged in, so room history can say where it has holes
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  logged_in_at TEXT NOT NULL,
  -- moved on every minute while logged in
  last_seen_at TEXT NOT NULL,
  -- null while the session runs
  ended_at TEXT
);

-- the chats pinned above the others in the chat list, rooms and users in one order of the
-- user's. a pin outlives leaving the room or closing the conversation, as `room_state` and
-- `conversation_state` do, and shows again when the chat is back
CREATE TABLE chat_pins (
  kind TEXT NOT NULL CHECK (kind IN ('room', 'user')),
  -- a room's name or a username, exact and case-sensitive
  name TEXT NOT NULL,
  -- the pin's place in the order, the lowest first
  position INTEGER NOT NULL,
  PRIMARY KEY (kind, name)
);

-- the revision of the pinned chats, one more with every change of the list, so a client can tell
-- an older list from a newer one whatever order it hears of them in
CREATE TABLE chat_pins_revision (
  -- the only row
  id INTEGER PRIMARY KEY CHECK (id = 0),
  revision INTEGER NOT NULL
);
INSERT INTO chat_pins_revision (id, revision) VALUES (0, 0);

-- whether we set ourselves away, kept so a restart keeps it; an away that came from nobody
-- having the UI open is not kept
CREATE TABLE own_status (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  away INTEGER NOT NULL
);

-- the full-text indexes of the messages, kept by the triggers below; the text is read from the
-- message tables, so search hits can show the matching part
CREATE VIRTUAL TABLE private_messages_fts USING fts5(
  message, content = 'private_messages', content_rowid = 'local_id', tokenize = 'unicode61'
);
CREATE TRIGGER private_messages_indexed AFTER INSERT ON private_messages BEGIN
  INSERT INTO private_messages_fts (rowid, message) VALUES (new.local_id, new.message);
END;
CREATE TRIGGER private_messages_unindexed AFTER DELETE ON private_messages BEGIN
  INSERT INTO private_messages_fts (private_messages_fts, rowid, message)
  VALUES ('delete', old.local_id, old.message);
END;

CREATE VIRTUAL TABLE room_messages_fts USING fts5(
  message, content = 'room_messages', content_rowid = 'local_id', tokenize = 'unicode61'
);
CREATE TRIGGER room_messages_indexed AFTER INSERT ON room_messages BEGIN
  INSERT INTO room_messages_fts (rowid, message) VALUES (new.local_id, new.message);
END;
CREATE TRIGGER room_messages_unindexed AFTER DELETE ON room_messages BEGIN
  INSERT INTO room_messages_fts (room_messages_fts, rowid, message)
  VALUES ('delete', old.local_id, old.message);
END;
