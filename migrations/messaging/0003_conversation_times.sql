-- nothing reads when a conversation was made or last changed
ALTER TABLE conversations DROP COLUMN created_at;
ALTER TABLE conversations DROP COLUMN updated_at;
