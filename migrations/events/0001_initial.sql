-- every raised event, kept until retention removes it
CREATE TABLE Events (
  Id TEXT PRIMARY KEY,
  -- when it happened, UTC RFC 3339 with microseconds, so text order is time order
  Timestamp TEXT NOT NULL,
  -- the type's name, such as DownloadFileComplete
  Type TEXT NOT NULL,
  -- the user the event is about, from its data, for filtering
  Username TEXT,
  -- the event's data as a JSON object
  Data TEXT
);
CREATE INDEX IX_Events_Timestamp ON Events(Timestamp);
CREATE INDEX IX_Events_Type ON Events(Type);
CREATE INDEX IX_Events_Username ON Events(Username);
