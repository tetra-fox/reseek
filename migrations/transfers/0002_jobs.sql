-- the job a download belongs to: everything queued in one go, such as a file, a folder or an
-- album, told of once when the last of its downloads has ended. null on uploads and on
-- downloads queued before jobs were kept
ALTER TABLE Transfers ADD COLUMN JobId TEXT;
CREATE INDEX IDX_Transfers_JobId ON Transfers(JobId);
