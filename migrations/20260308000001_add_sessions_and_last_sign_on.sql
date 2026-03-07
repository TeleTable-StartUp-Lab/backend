-- Add last_sign_on for users
ALTER TABLE users
ADD COLUMN IF NOT EXISTS last_sign_on TIMESTAMP WITH TIME ZONE;

-- Create sessions table for full login session history
CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip_address TEXT NOT NULL,
    fingerprint_data JSONB NOT NULL DEFAULT '{}'::jsonb,
    user_agent TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    is_current BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_created_at ON sessions(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_user_id_created_at ON sessions(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_current ON sessions(user_id, is_current);
