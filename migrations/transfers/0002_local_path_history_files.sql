-- the absolute path a download's file was moved to once it arrived
ALTER TABLE Transfers ADD COLUMN LocalPath TEXT;
-- when the transfer was taken out of the listings, so removed rows can be deleted for good
-- once they have been removed long enough
ALTER TABLE Transfers ADD COLUMN RemovedAt TEXT;

-- the states each transfer entered, in the order it entered them; the newest 64 per transfer
-- are kept, and a transfer's rows go when the transfer is deleted
CREATE TABLE transfers_state_history (
  transfer_id TEXT NOT NULL,
  state INTEGER NOT NULL,
  at TEXT NOT NULL
);
CREATE INDEX IDX_transfers_state_history_transfer_id ON transfers_state_history(transfer_id);

-- the files a batch was enqueued with, as a JSON array of their names, sizes and attributes,
-- kept to show beside the downloads
ALTER TABLE Batches ADD COLUMN files_json TEXT;
