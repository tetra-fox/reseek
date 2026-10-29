-- the shape the report's JSON was written in, which the daemon reads only in its own shape; null
-- for a report stored before shapes were numbered, which is read when it can be and is stale
-- otherwise
ALTER TABLE reports ADD COLUMN format INTEGER;
