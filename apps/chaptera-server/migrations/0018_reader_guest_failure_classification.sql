-- CLOUD-READER-CORPUS-PROMOTION-01B current-main reconciliation
-- Persist the bounded server-owned terminal failure classification so repeated
-- open/scene responses keep the same NOT_PUB / damaged / high-value evidence.
ALTER TABLE reader_guest_sessions
    ADD COLUMN failure_classification_json BLOB
    CHECK (
        failure_classification_json IS NULL OR
        length(failure_classification_json) <= 8192
    );
