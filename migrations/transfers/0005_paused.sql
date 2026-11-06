-- 1 on a download the user paused: it waits here, keeping what arrived, until resumed, and a
-- stop leaves it as it is
ALTER TABLE Transfers ADD COLUMN Paused INTEGER NOT NULL DEFAULT 0;
