-- migrations/{timestamp}_create_subscription_token_table.sql
-- Create Subscriptions Token Table
CREATE TABLE subscription_tokens(
    subscription_token TEXT NOT NULL,
    subscriber_id uuid NOT NULL REFERENCES subscriptions (id),
    PRIMARY KEY (subscription_token)
);