-- a session that began where client events were lost on their way to the messaging engine,
-- not at a login: the gap before it is listed however short it is
ALTER TABLE sessions ADD COLUMN after_loss INTEGER NOT NULL DEFAULT 0;
