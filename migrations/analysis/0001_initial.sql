-- the newest analysis of each file, by the file's absolute path
CREATE TABLE reports (
  path TEXT PRIMARY KEY,
  -- "downloads" or "incomplete", and the path under that directory, `/`-separated
  root TEXT NOT NULL,
  relative TEXT NOT NULL,
  -- the file as it was analysed; a different size or modification time is a different file
  size INTEGER NOT NULL,
  modified TEXT NOT NULL,
  -- the download the file belongs to, when it was found as one
  transfer_id TEXT,
  -- UTC RFC 3339 with microseconds, so text order is time order
  analysed_at TEXT NOT NULL,
  -- the analyser's reading, for listing by it
  verdict TEXT NOT NULL,
  -- the name the spectrogram images are kept under in the analysis directory
  image TEXT NOT NULL,
  -- the report as JSON, with its windows and without the user's judgement
  report TEXT NOT NULL
);
CREATE INDEX reports_analysed_at ON reports(analysed_at);
CREATE INDEX reports_transfer_id ON reports(transfer_id);

-- the user's own judgement of a file, kept apart from its report so analysing the file again
-- keeps it
CREATE TABLE judgements (
  path TEXT PRIMARY KEY,
  -- "legit" or "fake"
  verdict TEXT NOT NULL,
  note TEXT,
  -- the subject of the principal that recorded it
  by TEXT NOT NULL,
  at TEXT NOT NULL
);

-- the spectrogram images kept in the analysis directory, for removing the least recently
-- viewed when they take more room than allowed
CREATE TABLE images (
  name TEXT PRIMARY KEY,
  bytes INTEGER NOT NULL,
  used_at TEXT NOT NULL
);
CREATE INDEX images_used_at ON images(used_at);
