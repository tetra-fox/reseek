-- when a schedule's last run ended, which the card shows how long it took from
ALTER TABLE schedules ADD COLUMN ended_at TEXT;
