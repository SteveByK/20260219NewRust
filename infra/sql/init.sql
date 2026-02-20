CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS users (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  username text UNIQUE NOT NULL,
  password_hash text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_locations (
  user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  location geography(Point, 4326) NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS room_messages (
  id bigserial PRIMARY KEY,
  room_id text NOT NULL,
  from_user uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  message text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS room_member_reads (
  room_id text NOT NULL,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  last_read_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (room_id, user_id)
);

CREATE TABLE IF NOT EXISTS invites (
  id uuid PRIMARY KEY,
  from_user uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  to_user uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  mode text NOT NULL,
  status text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  responded_at timestamptz
);

CREATE INDEX IF NOT EXISTS idx_user_locations_gist
  ON user_locations USING GIST (location);

CREATE INDEX IF NOT EXISTS idx_room_messages_room_time
  ON room_messages (room_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_invites_to_status
  ON invites (to_user, status, created_at DESC);
