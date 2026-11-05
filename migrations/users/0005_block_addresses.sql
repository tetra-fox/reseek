-- the addresses blocked with a user, a JSON array of IP addresses, networks or ranges; anyone
-- connecting from one of them is blocked from the block's scopes
ALTER TABLE blocks ADD COLUMN addresses TEXT NOT NULL DEFAULT '[]';
