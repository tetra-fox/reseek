-- every job: the work running, paused or waiting, and the ended ones kept as history for
-- retention.jobs; times are UTC RFC 3339 with microseconds, so text order is time order
CREATE TABLE jobs (
  id TEXT PRIMARY KEY,
  -- download, scan, analysis or maintenance
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  -- you, schedule, download or resumed, with the schedule's name
  starter TEXT NOT NULL,
  starter_name TEXT,
  -- queued, running, paused or ended
  state TEXT NOT NULL,
  -- succeeded, finished_with_failures, failed or canceled once ended
  ending TEXT,
  error TEXT,
  summary TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  ended_at TEXT,
  total INTEGER NOT NULL DEFAULT 0,
  succeeded INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  canceled INTEGER NOT NULL DEFAULT 0,
  paused INTEGER NOT NULL DEFAULT 0,
  -- the download job an analysis job analyzes the files of
  follows TEXT,
  -- what a run again repeats, as the kind reads it
  data TEXT
);
CREATE INDEX jobs_state ON jobs(state);
CREATE INDEX jobs_ended_at ON jobs(ended_at);
CREATE INDEX jobs_follows ON jobs(follows);

-- the parts of a job: the files of an analysis job, waiting or done, and the parts of an ended
-- job by name, state and error; a running download job's parts are its downloads
CREATE TABLE job_parts (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  position INTEGER NOT NULL,
  name TEXT NOT NULL,
  -- waiting, running, paused, succeeded, failed or canceled
  state TEXT NOT NULL,
  error TEXT,
  -- what the part works on, as its kind reads it
  target TEXT,
  -- what finds the part among every job's, such as an analyzed file's local path
  key TEXT
);
CREATE INDEX job_parts_job ON job_parts(job_id, position);
CREATE INDEX job_parts_key ON job_parts(key);
CREATE INDEX job_parts_state ON job_parts(state);

-- the last run of each schedule, for its card and its next run
CREATE TABLE schedules (
  name TEXT PRIMARY KEY,
  -- the last run the schedule itself started, which the next one is counted from
  scheduled_at TEXT,
  -- the last run of any starter
  run_at TEXT,
  ending TEXT,
  error TEXT,
  summary TEXT,
  job_id TEXT
);
