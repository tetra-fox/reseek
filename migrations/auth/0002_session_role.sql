-- a session's role is read from the configuration on each request, so it is not kept
ALTER TABLE sessions DROP COLUMN role;
-- the allowed emails are checked against an identity provider's verified email only, which
-- earlier sign-ins did not keep apart from an unverified one
UPDATE identities SET email = NULL WHERE provider LIKE 'oidc:%';
