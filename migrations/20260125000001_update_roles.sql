-- Update default role to 'Viewer'
ALTER TABLE users ALTER COLUMN role SET DEFAULT 'Viewer';

-- Migrate existing 'user' roles to 'Viewer' (if any exist and we want to standardize)
UPDATE users SET role = 'Viewer' WHERE role = 'user';

-- Optional: Add a check constraint to enforce valid roles
-- ALTER TABLE users ADD CONSTRAINT check_valid_roles CHECK (role IN ('Admin', 'Operator', 'Viewer'));
