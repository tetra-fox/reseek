-- a row in a state other than those the daemon writes, listed below, or completed with no end,
-- fails with "State lost" at its last known time; 272 is `Completed, Errored`, 64 the history kept
CREATE TEMP TABLE lost AS
SELECT Id, coalesce(EndedAt, nullif(max(
  RequestedAt,
  coalesce(EnqueuedAt, ''),
  coalesce(StartedAt, ''),
  coalesce((SELECT max(at) FROM transfers_state_history WHERE transfer_id = Transfers.Id), '')
), ''), strftime('%Y-%m-%dT%H:%M:%f000Z', 'now')) AS ended
FROM Transfers
WHERE State NOT IN (1, 2050, 4098, 4, 8, 48, 80, 144, 272, 528, 1040)
  OR (State IN (48, 80, 144, 272, 528, 1040) AND EndedAt IS NULL);
INSERT INTO transfers_state_history (transfer_id, state, at) SELECT Id, 272, ended FROM lost;
DELETE FROM transfers_state_history WHERE rowid IN (
  SELECT rowid FROM (
    SELECT rowid, row_number() OVER (PARTITION BY transfer_id ORDER BY rowid DESC) AS newer
    FROM transfers_state_history WHERE transfer_id IN (SELECT Id FROM lost)
  ) WHERE newer > 64
);
UPDATE Transfers SET State = 272, StateDescription = 'Completed, Errored', EndedAt = lost.ended,
  Exception = 'State lost', NextAttemptAt = NULL, PlaceInQueue = NULL, Paused = 0, Resumable = 0
FROM lost WHERE Transfers.Id = lost.Id;
DROP TABLE lost;
