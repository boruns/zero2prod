-- migrations/{timestamp}_create_users_table.sql
-- Create Users Table
CREATE TABLE users(
    user_id uuid PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL
);