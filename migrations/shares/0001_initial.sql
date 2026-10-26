-- one row per scan: when it started, the share options it ran with, when it ended, and
-- whether the index may since have drifted from what is on disk; times are epoch milliseconds
CREATE TABLE scans (
  timestamp INTEGER PRIMARY KEY,
  options TEXT NOT NULL,
  "end" INTEGER DEFAULT NULL,
  suspect INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_scans_timestamp ON scans(timestamp DESC);

-- every shared directory by its remote name, such as 'Music\Artist\Album', each share's own
-- directory included, with the scan or change that last saw it
CREATE TABLE directories (
  name TEXT PRIMARY KEY,
  timestamp INTEGER NOT NULL
);
CREATE INDEX idx_directories_timestamp ON directories(timestamp);

-- every shared file by its remote name, with the local path's bytes as the system gives them,
-- its size and modification time as last read, and the attributes read from it
CREATE TABLE files (
  -- the file's row in the filename index
  id INTEGER PRIMARY KEY,
  maskedFilename TEXT NOT NULL UNIQUE,
  originalFilename BLOB NOT NULL,
  size INTEGER NOT NULL,
  -- the last write time in epoch milliseconds
  touchedAt INTEGER NOT NULL,
  code INTEGER NOT NULL DEFAULT 1,
  extension TEXT NOT NULL,
  -- a JSON array of { "type", "value" }, in the order they are sent
  attributeJson TEXT NOT NULL,
  timestamp INTEGER NOT NULL
);
CREATE INDEX idx_files_timestamp ON files(timestamp);

-- the words of every remote name for searches, each row keyed by its file's id
CREATE VIRTUAL TABLE filenames USING fts5(
  maskedFilename,
  tokenize = 'unicode61 remove_diacritics 1'
);
