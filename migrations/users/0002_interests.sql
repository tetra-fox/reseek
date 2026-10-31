-- what we like (liked = 1) and hate (liked = 0), in the order we listed them; the server is told
-- them at every login
CREATE TABLE interests (
  interest TEXT PRIMARY KEY,
  liked INTEGER NOT NULL
);
