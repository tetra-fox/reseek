-- a row in a state no build writes, or completed with no end time, ends as `Completed, Errored`
-- with "State lost", so the listings show it as failed and retention takes it by its age. It
-- ends when it ended if that is known, else at the latest time it holds, else now. The values
-- are those the daemon writes to `State`; 272 is `Completed, Errored`
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
UPDATE Transfers SET State = 272, StateDescription = 'Completed, Errored', EndedAt = lost.ended,
  Exception = 'State lost', NextAttemptAt = NULL, PlaceInQueue = NULL, Paused = 0
FROM lost WHERE Transfers.Id = lost.Id;
DROP TABLE lost;
