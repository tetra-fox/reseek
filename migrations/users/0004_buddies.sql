-- the flag that listed a user first is the buddy flag; a buddy may also be trusted, and we may
-- ask to be told when their presence changes
ALTER TABLE users RENAME COLUMN pinned TO buddy;
DROP INDEX users_pinned;
CREATE INDEX users_buddy ON users(buddy) WHERE buddy = 1;
ALTER TABLE users ADD COLUMN trusted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN notify_presence INTEGER NOT NULL DEFAULT 0;
