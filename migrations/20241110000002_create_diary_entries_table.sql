-- Create diary_entries table
CREATE TABLE IF NOT EXISTS diary_entries (
    id UUID PRIMARY KEY,
    owner UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    working_minutes INTEGER NOT NULL,
    text TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Create index on owner for faster lookups
CREATE INDEX IF NOT EXISTS idx_diary_entries_owner ON diary_entries(owner);

-- Create index on created_at for sorting
CREATE INDEX IF NOT EXISTS idx_diary_entries_created_at ON diary_entries(created_at);
