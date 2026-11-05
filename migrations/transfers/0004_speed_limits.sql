-- which speed limits are in force, one row; none means the normal ones
CREATE TABLE speed_limits (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  alternative INTEGER NOT NULL
);
