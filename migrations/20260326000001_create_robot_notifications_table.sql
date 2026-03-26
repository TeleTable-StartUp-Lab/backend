CREATE TABLE IF NOT EXISTS robot_notifications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    priority TEXT NOT NULL CHECK (priority IN ('INFO', 'WARN', 'ERROR')),
    message TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_robot_notifications_received_at
    ON robot_notifications (received_at DESC);
