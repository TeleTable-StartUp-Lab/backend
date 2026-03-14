CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- keep diary_entries.updated_at authoritative on every update
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS diary_entries_updated_at ON diary_entries;
CREATE TRIGGER diary_entries_updated_at
BEFORE UPDATE ON diary_entries
FOR EACH ROW
EXECUTE FUNCTION set_updated_at();

-- PK UUID defaults for safer manual inserts/scripts
ALTER TABLE users
    ALTER COLUMN id SET DEFAULT gen_random_uuid();

ALTER TABLE diary_entries
    ALTER COLUMN id SET DEFAULT gen_random_uuid();

ALTER TABLE sessions
    ALTER COLUMN id SET DEFAULT gen_random_uuid();

ALTER TABLE diary_entries
    ADD CONSTRAINT diary_entries_text_length_check
    CHECK (char_length(text) <= 5000);

-- remove redundant sign-on state from users and derive from sessions
ALTER TABLE users
    DROP COLUMN last_sign_on;

CREATE VIEW user_last_sign_on AS
SELECT user_id, MAX(created_at) AS last_sign_on
FROM sessions
GROUP BY user_id;

-- remove mutable current-session flag and derive current session via latest row
ALTER TABLE sessions
    DROP COLUMN is_current;

CREATE VIEW current_sessions AS
SELECT DISTINCT ON (user_id) *
FROM sessions
ORDER BY user_id, created_at DESC;

-- add constrain so to ensure roles are only one of the three roles in our RBAC
ALTER TABLE users ADD CONSTRAINT check_valid_roles CHECK (role IN ('Admin', 'Operator', 'Viewer'));
