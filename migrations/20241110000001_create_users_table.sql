-- Create users table
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL DEFAULT 'user',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Create index on email for faster lookups
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

-- Insert an admin user record
INSERT INTO users (
    id,
    name,
    email,
    password_hash,
    role
) VALUES (
    gen_random_uuid(),
    'Admin User',
    'admin@teletable.com',
    -- Replace 'your_secure_hashed_password' with the result of hashing the actual password.
    '$2a$12$V86w3et3599PcMD42jh0SeryztjyS9UglXvGz.rp/dHbHSZcXSQsu', 
    'admin' -- Sets the role to 'admin'
);
