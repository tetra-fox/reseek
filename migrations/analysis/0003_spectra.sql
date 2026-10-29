-- a report's spectrogram and waveform are kept as data in one file, named in `spectra`
ALTER TABLE reports RENAME COLUMN image TO spectra;
DROP TABLE images;

-- the spectrogram and waveform files kept in the analysis directory, for removing the least
-- recently viewed when they take more room than allowed
CREATE TABLE spectra (
  name TEXT PRIMARY KEY,
  bytes INTEGER NOT NULL,
  used_at TEXT NOT NULL
);
CREATE INDEX spectra_used_at ON spectra(used_at);
