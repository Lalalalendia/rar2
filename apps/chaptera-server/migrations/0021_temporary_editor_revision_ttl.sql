-- Temporary Edit mode lifetime, composed with the existing RevisionStream.
-- Does not create another revision authority or persist an independent Project.
-- Server-owned document_id is exactly the private tempedit session identity.
-- A committed *new* RevisionStream edge (including Undo/Redo) is the only
-- permitted event which extends the one-hour expiry. Reader GET/export does not.
CREATE TRIGGER IF NOT EXISTS tempedit_revision_accept_guard
BEFORE INSERT ON revision_edges
WHEN substr(CAST(NEW.document_id AS TEXT), 1, 9) = 'tempedit:'
BEGIN
    SELECT CASE
        WHEN length(CAST(NEW.document_id AS TEXT)) != 41
          OR substr(CAST(NEW.document_id AS TEXT), 10) GLOB '*[^0-9a-f]*'
          OR NEW.committed_at_ms > 9223372036851175807
          OR NOT EXISTS (
            SELECT 1 FROM reader_guest_sessions AS guest
            WHERE guest.session_id = NEW.document_id
              AND guest.state = 'opened'
              AND guest.classification IN ('supported', 'partial')
              AND guest.source_sha256 IS NOT NULL
              AND guest.quarantine_deleted_at_ms IS NULL
              AND guest.updated_at_ms <= NEW.committed_at_ms
              AND guest.expires_at_ms > NEW.committed_at_ms
              AND guest.expires_at_ms > CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
          )
        THEN RAISE(ABORT, 'tempedit_revision_session_not_writable')
    END;
END;

CREATE TRIGGER IF NOT EXISTS tempedit_revision_commit_renews
AFTER INSERT ON revision_edges
WHEN substr(CAST(NEW.document_id AS TEXT), 1, 9) = 'tempedit:'
BEGIN
    UPDATE reader_guest_sessions
    SET expires_at_ms = NEW.committed_at_ms + 3600000,
        updated_at_ms = NEW.committed_at_ms
    WHERE session_id = NEW.document_id AND state = 'opened';
END;

-- The existing Guest Reader sweeper deletes quarantine *before* deleting the
-- session row. Once its exact deletion succeeds, no private revision or
-- canonical-identity payload may remain reachable through this document_id.
CREATE TRIGGER IF NOT EXISTS tempedit_session_delete_revisions
AFTER DELETE ON reader_guest_sessions
WHEN substr(CAST(OLD.session_id AS TEXT), 1, 9) = 'tempedit:'
BEGIN
    DELETE FROM revision_identity_bindings WHERE document_id = OLD.session_id;
    DELETE FROM revision_edges WHERE document_id = OLD.session_id;
END;
