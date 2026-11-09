-- the groups we put users in, by name: a user-defined group of the configuration, or the name
-- of one it no longer has, kept until the members are moved or let go
CREATE TABLE memberships (
  username TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE,
  group_name TEXT NOT NULL,
  PRIMARY KEY (username, group_name)
);
CREATE INDEX memberships_group ON memberships(group_name);

-- a tag, the upload group we put a user in and a trusted buddy are each a membership
INSERT OR IGNORE INTO memberships (username, group_name)
  SELECT users.username, tag.value FROM users, json_each(users.tags) AS tag;
INSERT OR IGNORE INTO memberships (username, group_name)
  SELECT username, lower(group_override) FROM users WHERE group_override IS NOT NULL;
INSERT OR IGNORE INTO memberships (username, group_name)
  SELECT username, 'trusted' FROM users WHERE buddy = 1 AND trusted = 1;

ALTER TABLE users DROP COLUMN tags;
ALTER TABLE users DROP COLUMN group_override;
ALTER TABLE users DROP COLUMN trusted;
