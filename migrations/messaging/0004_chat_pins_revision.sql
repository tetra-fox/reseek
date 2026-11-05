-- the revision of the pinned chats, one more with every change of the list, so a client can tell
-- an older list from a newer one whatever order it hears of them in
CREATE TABLE chat_pins_revision (
  -- the only row
  id INTEGER PRIMARY KEY CHECK (id = 0),
  revision INTEGER NOT NULL
);
INSERT INTO chat_pins_revision (id, revision) VALUES (0, 0);
