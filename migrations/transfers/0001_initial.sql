-- groups of downloads enqueued together, such as one folder from a search
CREATE TABLE Batches (
  Id TEXT PRIMARY KEY,
  -- the search the batch was picked from; searches live in their own database
  SearchId TEXT,
  Username TEXT,
  -- "Download"; there are no upload batches
  Direction TEXT NOT NULL,
  CreatedAt TEXT NOT NULL,
  -- the batch's options as a JSON object, such as its destination
  Options TEXT,
  -- the files the batch was enqueued with, as a JSON array of their names, sizes and
  -- attributes, kept to show beside the downloads
  files_json TEXT
);
CREATE INDEX IDX_Batches_SearchId ON Batches(SearchId);

-- one row per file, peer and direction; rows are flagged removed, never deleted
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
  BatchId TEXT REFERENCES Batches(Id) ON DELETE SET NULL,
  Attempts INTEGER NOT NULL DEFAULT 0,
  NextAttemptAt TEXT,
  -- the absolute path a download's file was moved to once it arrived
  LocalPath TEXT,
  -- when the transfer was taken out of the listings, so removed rows can be deleted for good
  -- once they have been removed long enough
  RemovedAt TEXT,
  -- 1 on a download a stop or the startup cleanup ended, to be enqueued again once after the
  -- next login; the login clears it whether the download could be enqueued or not
  Resumable INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IDX_Transfers_Direction ON Transfers(Direction);
CREATE INDEX IDX_Transfers_State ON Transfers(State);
CREATE INDEX IDX_Transfers_Removed ON Transfers(Removed);
CREATE INDEX IDX_Transfers_UsernameFilename ON Transfers(Username, Filename);
CREATE INDEX IDX_Transfers_UserUploadStatistics
  ON Transfers(Username, Direction, EndedAt, StartedAt, State, Size);
CREATE INDEX IDX_Transfers_BatchId ON Transfers(BatchId);
CREATE INDEX IDX_Transfers_DirectionRemovedState ON Transfers(Direction, Removed, State);

-- the states each transfer entered, in the order it entered them; the newest 64 per transfer
-- are kept, and a transfer's rows go when the transfer is deleted
CREATE TABLE transfers_state_history (
  transfer_id TEXT NOT NULL,
  state INTEGER NOT NULL,
  at TEXT NOT NULL
);
CREATE INDEX IDX_transfers_state_history_transfer_id ON transfers_state_history(transfer_id);
