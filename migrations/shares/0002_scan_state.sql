-- where each scan stands: 'running' while it runs, 'finished' once it completed, 'failed' when
-- an error stopped it and 'canceled' when it was asked to stop; only a finished scan is trusted
ALTER TABLE scans ADD COLUMN state TEXT NOT NULL DEFAULT 'running'
  CHECK (state IN ('running', 'finished', 'failed', 'canceled'));

-- no scan runs while the index is migrated, so a scan without an end stopped without completing
UPDATE scans SET state = CASE WHEN "end" IS NULL THEN 'failed' ELSE 'finished' END;
