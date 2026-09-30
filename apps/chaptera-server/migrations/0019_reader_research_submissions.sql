-- CLOUD-READER-CORPUS-PROMOTION-01D
-- Separate explicit-consent records from ephemeral guest sessions.
-- No filenames, paths, source bytes or client-authoritative hashes are stored.
CREATE TABLE IF NOT EXISTS reader_research_submissions (
    submission_id                 BLOB PRIMARY KEY,
    session_id                    BLOB NOT NULL,
    capability_token_hash         BLOB NOT NULL UNIQUE CHECK (length(capability_token_hash) = 32),
    consent_version               TEXT NOT NULL,
    retention_policy              TEXT NOT NULL,
    failure_classification_json   BLOB NOT NULL CHECK (length(failure_classification_json) <= 8192),
    failure_code                  TEXT NOT NULL,
    state                         TEXT NOT NULL CHECK (
        state IN ('issued', 'consuming', 'retained')
    ),
    issued_at_ms                  INTEGER NOT NULL CHECK (issued_at_ms >= 0),
    expires_at_ms                 INTEGER NOT NULL CHECK (expires_at_ms > issued_at_ms),
    consumed_at_ms                INTEGER,
    binding_id                    BLOB,
    server_sha256                 BLOB,
    exact_byte_disposition        TEXT CHECK (
        exact_byte_disposition IS NULL OR
        exact_byte_disposition IN ('new_exact_bytes', 'duplicate_exact_bytes')
    ),
    cluster_disposition           TEXT CHECK (
        cluster_disposition IS NULL OR cluster_disposition = 'deferred'
    ),
    deletion_requested_at_ms      INTEGER,
    deleted_at_ms                 INTEGER,

    CHECK (length(submission_id) BETWEEN 16 AND 160),
    CHECK (length(session_id) BETWEEN 1 AND 128),
    CHECK (length(consent_version) BETWEEN 1 AND 96),
    CHECK (length(retention_policy) BETWEEN 1 AND 96),
    CHECK (length(failure_code) BETWEEN 1 AND 96),
    CHECK (server_sha256 IS NULL OR length(server_sha256) = 64),
    CHECK (
        state != 'retained' OR (
            consumed_at_ms IS NOT NULL AND
            binding_id IS NOT NULL AND
            server_sha256 IS NOT NULL AND
            exact_byte_disposition IS NOT NULL AND
            cluster_disposition IS NOT NULL
        )
    ),
    CHECK (deleted_at_ms IS NULL OR deletion_requested_at_ms IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS reader_research_submissions_session_idx
    ON reader_research_submissions (session_id, issued_at_ms);

CREATE INDEX IF NOT EXISTS reader_research_submissions_state_idx
    ON reader_research_submissions (state, expires_at_ms);
