-- the remote folder a download was asked for in, whose tree below takes the place of the
-- download pattern's ${SOURCE_DIRECTORY}; null for a file that goes to its own folder's name
ALTER TABLE Transfers ADD COLUMN Folder TEXT;
