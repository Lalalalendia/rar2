-- CLOUD-ABUSE-PUBLIC-01
-- Restart-safe anonymous/public request-rate state.
--
-- subject_key is a server-keyed SHA-256 pseudonym of the trusted edge ClientIp.
-- Raw network addresses, request bodies, filenames and PUB-derived values never
-- belong in this authority.

CREATE TABLE IF NOT EXISTS public_rate_limit_state (
    subject_key             BLOB NOT NULL CHECK (length(subject_key) = 32),
    policy_class            TEXT NOT NULL,
    theoretical_arrival_us  INTEGER NOT NULL CHECK (theoretical_arrival_us >= 0),
    last_seen_at_ms         INTEGER NOT NULL CHECK (last_seen_at_ms >= 0),

    PRIMARY KEY (subject_key, policy_class),
    CHECK (length(policy_class) BETWEEN 1 AND 64)
);

CREATE INDEX IF NOT EXISTS public_rate_limit_cleanup_idx
    ON public_rate_limit_state (last_seen_at_ms);
