-- PUB-COMPAT-EXPORT-ROUTE-01
-- Persist only the source-safe, worker-owned per-file editable-route assessment.
-- Raw PUB bytes, Story text, internal feature keys, and target artifacts do not
-- cross this persistence boundary.
ALTER TABLE reader_guest_sessions
    ADD COLUMN editable_routes_json BLOB
    CHECK (
        editable_routes_json IS NULL OR
        length(editable_routes_json) <= 4096
    );
