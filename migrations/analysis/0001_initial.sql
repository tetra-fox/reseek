-- the newest analysis of each file, by the file's absolute path
CREATE TABLE reports (
  path TEXT PRIMARY KEY,
  -- "downloads" or "incomplete", and the path under that directory, `/`-separated
  root TEXT NOT NULL,
  relative TEXT NOT NULL,
  -- the file as it was analyzed; a different size or modification time is a different file
  size INTEGER NOT NULL,
  modified TEXT NOT NULL,
  -- the download the file belongs to, when it was found as one
  transfer_id TEXT,
  -- UTC RFC 3339 with microseconds, so text order is time order
  analyzed_at TEXT NOT NULL,
  -- the analyzer's reading, for listing by it
  verdict TEXT NOT NULL,
  -- the name the report's spectrogram and waveform are kept under in the analysis directory,
  -- as a row of `spectra`
  spectra TEXT NOT NULL,
  -- the report as JSON, with its windows and without the user's judgment
  report TEXT NOT NULL,
  -- the shape the report's JSON was written in, which the daemon reads only in its own shape
  format INTEGER NOT NULL
);
CREATE INDEX reports_analyzed_at ON reports(analyzed_at);
CREATE INDEX reports_transfer_id ON reports(transfer_id);

-- the user's own judgment of a file, kept apart from its report so analyzing the file again
-- keeps it
CREATE TABLE judgments (
  path TEXT PRIMARY KEY,
  -- "legit" or "fake"
  verdict TEXT NOT NULL,
  note TEXT,
  -- the subject of the principal that recorded it
  by TEXT NOT NULL,
  at TEXT NOT NULL
);

-- the spectrogram and waveform files kept in the analysis directory, for removing the least
-- recently viewed when they take more room than allowed
CREATE TABLE spectra (
  name TEXT PRIMARY KEY,
  bytes INTEGER NOT NULL,
  used_at TEXT NOT NULL
);
CREATE INDEX spectra_used_at ON spectra(used_at);
