-- the chats pinned above the others in the chat list, rooms and users in one order of the
-- user's. a pin outlives leaving the room or closing the conversation, as `room_state` and
-- `conversation_state` do, and shows again when the chat is back
CREATE TABLE chat_pins (
  -- "room" or "user"
  kind TEXT NOT NULL,
  -- a room's name or a username, exact and case-sensitive
  name TEXT NOT NULL,
  -- the pin's place in the order, the lowest first
  position INTEGER NOT NULL,
  PRIMARY KEY (kind, name)
);
