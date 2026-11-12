-- the responses of a search that still runs, saved in batches as it takes them, so a stop that
-- does not end it keeps them; its end moves them into `searches.responses`
CREATE TABLE running_responses (
  search_id TEXT NOT NULL,
  -- how many responses the search had saved before this batch
  first INTEGER NOT NULL,
  -- a zlib-compressed JSON array, as `searches.responses`
  responses BLOB NOT NULL,
  PRIMARY KEY (search_id, first)
) WITHOUT ROWID;
