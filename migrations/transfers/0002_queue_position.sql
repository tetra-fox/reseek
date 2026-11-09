-- where each download stands in the local queue, lower starting first, so an order the user set
-- is kept across restarts; a download enqueued again after a restart keeps its place. null on
-- uploads. The downloads written before this take the order they were asked in
ALTER TABLE Transfers ADD COLUMN QueuePosition REAL;
UPDATE Transfers SET QueuePosition = asked.position
FROM (
  SELECT rowid AS row, row_number() OVER (ORDER BY RequestedAt, rowid) AS position
  FROM Transfers WHERE Direction = 'Download'
) AS asked
WHERE Transfers.rowid = asked.row;
CREATE INDEX IDX_Transfers_QueuePosition ON Transfers(QueuePosition);
