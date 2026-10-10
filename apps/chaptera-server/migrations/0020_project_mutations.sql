-- CLOUD-PROJECT-LIFECYCLE-01
-- Durable idempotency ledger for project metadata/lifecycle mutations.
--
-- Project rows remain the product authority. This ledger records exact
-- mutation receipts so a lost HTTP ACK can be replayed without reapplying
-- a write or misclassifying success as stale concurrency.

CREATE TABLE IF NOT EXISTS project_mutations (
    tenant_id                    BLOB NOT NULL,
    project_id                   BLOB NOT NULL REFERENCES projects(project_id),
    request_id                   BLOB NOT NULL,
    operation                    TEXT NOT NULL CHECK (operation IN ('rename')),
    request_hash                 BLOB NOT NULL,
    result_lifecycle_generation  INTEGER NOT NULL CHECK (result_lifecycle_generation >= 0),
    result_metadata_version      INTEGER NOT NULL CHECK (result_metadata_version >= 0),
    result_name                  TEXT NOT NULL,
    committed_at_ms              INTEGER NOT NULL,

    PRIMARY KEY (tenant_id, request_id),

    CHECK (length(tenant_id) > 0),
    CHECK (length(project_id) > 0),
    CHECK (length(request_id) > 0),
    CHECK (length(request_hash) = 64),
    CHECK (length(result_name) > 0),
    CHECK (length(result_name) <= 512),
    CHECK (committed_at_ms >= 0)
);

CREATE INDEX IF NOT EXISTS project_mutations_project_idx
    ON project_mutations (tenant_id, project_id, committed_at_ms);
