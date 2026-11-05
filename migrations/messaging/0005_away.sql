-- 1 for a message we sent on our own, as the answer to the first message from someone while we
-- were away
ALTER TABLE private_messages ADD COLUMN auto_reply INTEGER NOT NULL DEFAULT 0;

-- whether we set ourselves away, kept so a restart keeps it; an away that came from nobody
-- having the UI open is not kept
CREATE TABLE own_status (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  away INTEGER NOT NULL
);
