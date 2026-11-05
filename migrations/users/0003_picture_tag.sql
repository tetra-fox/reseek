-- the SHA-256 of `picture` in hex, null exactly when `picture` is; the URL the picture is
-- fetched from carries it, so a new picture is a new URL
ALTER TABLE users ADD COLUMN picture_tag TEXT;
