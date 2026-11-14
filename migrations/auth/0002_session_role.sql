-- a session's role is read from the configuration on each request, so it is not kept
ALTER TABLE sessions DROP COLUMN role;
