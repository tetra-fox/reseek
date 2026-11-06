-- one row per file, peer and direction; taking a transfer out of the listings flags it removed,
-- and a removed row is deleted for good once it has been removed long enough
CREATE TABLE Transfers (
  Id TEXT PRIMARY KEY,
  Username TEXT,
  -- "Download" or "Upload"
  Direction TEXT NOT NULL,
  -- the remote filename exactly as the peer gave it
  Filename TEXT,
  Size INTEGER NOT NULL,
  -- the state as its flag bits, and as their names for tools that read the file
  State INTEGER NOT NULL,
  StateDescription TEXT,
  -- times are UTC RFC 3339 with microseconds, so text order is time order
  RequestedAt TEXT NOT NULL,
  EnqueuedAt TEXT,
  StartedAt TEXT,
  EndedAt TEXT,
  BytesTransferred INTEGER NOT NULL,
  -- bytes per second
  AverageSpeed REAL NOT NULL,
  PlaceInQueue INTEGER,
  -- why it failed, without the transfer library's own prefix
  Exception TEXT,
  Removed INTEGER NOT NULL,
  Attempts INTEGER NOT NULL DEFAULT 0,
  NextAttemptAt TEXT,
  -- the absolute path a download's file was moved to once it arrived
  LocalPath TEXT,
  -- when the transfer was taken out of the listings
  RemovedAt TEXT,
  -- 1 on a download a stop or the startup cleanup ended, to be enqueued again once after the
  -- next login; the login clears it whether the download could be enqueued or not
  Resumable INTEGER NOT NULL DEFAULT 0,
  -- the job a download belongs to: everything queued in one go, such as a file, a folder or an
  -- album, told of once when the last of its downloads has ended. null on uploads
  JobId TEXT,
  -- the directory under the downloads directory a download's file goes to in place of the one
  -- the download pattern gives
  Destination TEXT,
  -- the search a download's file was found by; searches live in their own database
  SearchId TEXT,
  -- 1 on a download the user paused: it waits here, keeping what arrived, until resumed, and a
  -- stop leaves it as it is
  Paused INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IDX_Transfers_Direction ON Transfers(Direction);
CREATE INDEX IDX_Transfers_State ON Transfers(State);
CREATE INDEX IDX_Transfers_Removed ON Transfers(Removed);
CREATE INDEX IDX_Transfers_UsernameFilename ON Transfers(Username, Filename);
CREATE INDEX IDX_Transfers_UserUploadStatistics
  ON Transfers(Username, Direction, EndedAt, StartedAt, State, Size);
CREATE INDEX IDX_Transfers_DirectionRemovedState ON Transfers(Direction, Removed, State);
CREATE INDEX IDX_Transfers_JobId ON Transfers(JobId);

-- the states each transfer entered, in the order it entered them; the newest 64 per transfer
-- are kept, and a transfer's rows go when the transfer is deleted
CREATE TABLE transfers_state_history (
  transfer_id TEXT NOT NULL,
  state INTEGER NOT NULL,
  at TEXT NOT NULL
);
CREATE INDEX IDX_transfers_state_history_transfer_id ON transfers_state_history(transfer_id);

-- which speed limits are in force, one row; none means the normal ones
CREATE TABLE speed_limits (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  alternative INTEGER NOT NULL
);
