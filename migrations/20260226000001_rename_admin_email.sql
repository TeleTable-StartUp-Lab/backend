-- Rename admin email from admin@teletable.com to admin@teletable.net
UPDATE users
SET email = 'admin@teletable.net'
WHERE email = 'admin@teletable.com';
