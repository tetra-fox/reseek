-- 1 on a download a stop or the startup cleanup ended, to be enqueued again once after the
-- next login; the login clears it whether the download could be enqueued or not
ALTER TABLE Transfers ADD COLUMN Resumable INTEGER NOT NULL DEFAULT 0;
