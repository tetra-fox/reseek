-- who has signed in: one row per local account, identity provider user or proxy user
CREATE TABLE identities (
  subject TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  name TEXT,
  email TEXT,
  -- the groups the identity had at its last sign-in, as a JSON array
  claims_json TEXT,
  first_login_at TEXT NOT NULL,
  last_login_at TEXT NOT NULL
);

-- browser sessions; the cookie holds the id and an HMAC of it
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  subject TEXT NOT NULL REFERENCES identities(subject) ON DELETE CASCADE,
  role INTEGER NOT NULL,
  -- "local", "oidc:<provider>" or "proxy"
  via TEXT NOT NULL,
  -- the identity provider's session id and the sealed ID token, for its logout calls
  idp_sid TEXT,
  id_token TEXT,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  ip TEXT,
  user_agent TEXT,
  revoked_at TEXT
);
CREATE INDEX sessions_subject ON sessions(subject);
CREATE INDEX sessions_expiry ON sessions(expires_at);

-- API tokens; only a SHA-256 of the secret is kept
CREATE TABLE api_tokens (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  -- the first characters after "slsk_", to find the row and to tell tokens apart
  prefix TEXT NOT NULL,
  secret_hash BLOB NOT NULL,
  role INTEGER NOT NULL,
  owner_subject TEXT NOT NULL,
  -- comma-separated networks the token is accepted from, null for anywhere
  cidr TEXT,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  last_used_at TEXT,
  last_used_ip TEXT,
  revoked_at TEXT
);
CREATE INDEX api_tokens_prefix ON api_tokens(prefix);

-- the random key session cookies and sealed login state are made with, so sessions last
-- across restarts
CREATE TABLE keys (
  name TEXT PRIMARY KEY,
  secret BLOB NOT NULL,
  created_at TEXT NOT NULL
);
