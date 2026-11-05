-- a download keeps its own destination and the search it was found by, which its batch held
ALTER TABLE Transfers ADD COLUMN Destination TEXT;
ALTER TABLE Transfers ADD COLUMN SearchId TEXT;
UPDATE Transfers SET
  Destination = (SELECT json_extract(Options, '$.destination') FROM Batches
                 WHERE Batches.Id = Transfers.BatchId),
  SearchId = (SELECT SearchId FROM Batches WHERE Batches.Id = Transfers.BatchId)
WHERE BatchId IS NOT NULL;
DROP INDEX IDX_Transfers_BatchId;
ALTER TABLE Transfers DROP COLUMN BatchId;
DROP TABLE Batches;
