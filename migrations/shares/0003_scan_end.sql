-- a scan's end goes with its state: none while it runs, one once it finished; a scan that
-- failed or was canceled has one, except when a stop cut it off or it ran before the state was
-- recorded. SQLite adds a check only with the table, so the table is made again.
CREATE TABLE scans_checked (
  timestamp INTEGER PRIMARY KEY,
  options TEXT NOT NULL,
  "end" INTEGER DEFAULT NULL,
  suspect INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL DEFAULT 'running'
    CHECK (state IN ('running', 'finished', 'failed', 'canceled')),
  CHECK (CASE state
    WHEN 'running' THEN "end" IS NULL
    WHEN 'finished' THEN "end" IS NOT NULL
    ELSE 1
  END)
);
INSERT INTO scans_checked (timestamp, options, "end", suspect, state)
  SELECT timestamp, options, "end", suspect, state FROM scans;
DROP TABLE scans;
ALTER TABLE scans_checked RENAME TO scans;
CREATE INDEX idx_scans_timestamp ON scans(timestamp DESC);
