-- a conversation's messages in the order they were stored, so its newest one is a single seek
CREATE INDEX pm_user_local ON private_messages(username, local_id);
