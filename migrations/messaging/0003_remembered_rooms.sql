-- the rooms joined again after a start: a join asked for through the UI or the API marks the
-- room, a leave unmarks it; a room joined only because the configuration lists it is not marked
ALTER TABLE room_state ADD COLUMN remembered INTEGER NOT NULL DEFAULT 0;
