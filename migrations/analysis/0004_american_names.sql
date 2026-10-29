-- the column, its index and the judgment table take American spellings
ALTER TABLE reports RENAME COLUMN analysed_at TO analyzed_at;
DROP INDEX reports_analysed_at;
CREATE INDEX reports_analyzed_at ON reports(analyzed_at);
ALTER TABLE judgements RENAME TO judgments;
