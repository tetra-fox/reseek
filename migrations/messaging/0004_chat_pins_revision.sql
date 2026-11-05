-- the revision of the pinned chats, one more with every change of the list, so a client can tell
-- an older list from a newer one whatever order it hears of them in
CREATE TABLE chat_pins_revision (
  -- the only row
  id INTEGER PRIMARY KEY CHECK (id = 0),
  revision INTEGER NOT NULL
);
INSERT INTO chat_pins_revision (id, revision) VALUES (0, 0);

-- the pins again, with their kind checked, since a column's check cannot be added in place
CREATE TABLE chat_pins_checked (
  kind TEXT NOT NULL CHECK (kind IN ('room', 'user')),
  -- a room's name or a username, exact and case-sensitive
  name TEXT NOT NULL,
  -- the pin's place in the order, the lowest first
  position INTEGER NOT NULL,
  PRIMARY KEY (kind, name)
);
INSERT INTO chat_pins_checked (kind, name, position)
  SELECT kind, name, position FROM chat_pins;
DROP TABLE chat_pins;
ALTER TABLE chat_pins_checked RENAME TO chat_pins;
