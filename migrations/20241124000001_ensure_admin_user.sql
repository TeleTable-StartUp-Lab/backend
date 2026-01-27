-- Insert admin user if not exists
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
    '$2a$12$V86w3et3599PcMD42jh0SeryztjyS9UglXvGz.rp/dHbHSZcXSQsu', 
    'Admin'
) ON CONFLICT (email) DO NOTHING;
