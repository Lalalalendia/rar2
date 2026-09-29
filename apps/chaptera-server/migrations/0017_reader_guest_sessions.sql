-- CLOUD-READER-GUEST-INGRESS-01
-- Ephemeral anonymous Cloud Reader service sessions.
--
-- The seeded principal is an internal non-loginable control identity used only
-- to reuse the existing upload-admission byte/concurrency authority. It has no
-- principal_identity row, no session, no Workspace membership and no Project
-- authority.
INSERT OR IGNORE INTO principals (
    principal_id, created_at_ms, disabled_at_ms
) VALUES (
    CAST('principal:cloud-reader-guest-service' AS BLOB), 0, NULL
);

CREATE TABLE IF NOT EXISTS reader_guest_sessions (
    session_id                  BLOB PRIMARY KEY,
    access_token_hash           BLOB NOT NULL CHECK (length(access_token_hash) = 32),
    upload_id                   BLOB NOT NULL UNIQUE,
    reservation_id              BLOB NOT NULL UNIQUE,
    expected_byte_len           INTEGER NOT NULL CHECK (expected_byte_len > 0),
    observed_byte_len           INTEGER,
    storage_generation          BLOB,
    object_etag                 BLOB,
    source_sha256               BLOB,
    state                       TEXT NOT NULL CHECK (
        state IN ('issued', 'stored', 'opened', 'rejected', 'expired')
    ),
    classification              TEXT CHECK (
        classification IS NULL OR
        classification IN ('supported', 'partial', 'unsupported', 'rejected')
    ),
    scene_json                  BLOB,
    terminal_code               TEXT,
    created_at_ms               INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms               INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    expires_at_ms               INTEGER NOT NULL CHECK (expires_at_ms > created_at_ms),
    quarantine_deleted_at_ms    INTEGER,

    CHECK (length(session_id) BETWEEN 1 AND 128),
    CHECK (length(upload_id) BETWEEN 1 AND 128),
    CHECK (length(reservation_id) BETWEEN 1 AND 128),
    CHECK (observed_byte_len IS NULL OR observed_byte_len > 0),
    CHECK (source_sha256 IS NULL OR length(source_sha256) = 64),
    CHECK (scene_json IS NULL OR length(scene_json) <= 16777216),
    CHECK (
        quarantine_deleted_at_ms IS NULL OR
        quarantine_deleted_at_ms >= created_at_ms
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS reader_guest_sessions_token_idx
    ON reader_guest_sessions (access_token_hash);

CREATE INDEX IF NOT EXISTS reader_guest_sessions_expiry_idx
    ON reader_guest_sessions (expires_at_ms, quarantine_deleted_at_ms);

CREATE INDEX IF NOT EXISTS reader_guest_sessions_state_idx
    ON reader_guest_sessions (state, expires_at_ms);
