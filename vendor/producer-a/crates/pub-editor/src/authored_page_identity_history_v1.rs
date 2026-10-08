//! Durable authored Page identity history without page lifecycle semantics.
//!
//! Registering an identity does not add a Page to the canonical document or
//! mutate the immutable source graph. Active authored Page identities are
//! derived from Editor history, so Undo/Redo and project replay preserve the
//! exact caller-preallocated PageId.

use super::{EditOperation, EditorError, EditorSession};
use pub_editor_authoring_core::{
    AuthoredPageIdentityV1, validate_authored_page_identity_v1,
};
use pub_model::PageId;
use std::collections::BTreeMap;

impl EditorSession {
    pub fn authored_page_identities_v1(&self) -> BTreeMap<PageId, AuthoredPageIdentityV1> {
        self.undo
            .iter()
            .filter_map(|operation| match operation {
                EditOperation::RegisterAuthoredPageIdentityV1 { identity } => {
                    Some((identity.page_id, *identity))
                }
                _ => None,
            })
            .collect()
    }

    pub fn register_authored_page_identity_v1(
        &mut self,
        identity: AuthoredPageIdentityV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if validate_authored_page_identity_v1(&identity).is_err() {
            return Err(EditorError::AuthoredPageIdentityInvalid {
                page_id: identity.page_id,
            });
        }

        if self.graph.pages.contains_key(&identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            });
        }

        let operation = EditOperation::RegisterAuthoredPageIdentityV1 { identity };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthoredEntityProvenanceV1, EDITOR_PROJECT_VERSION_V0_24, EditorProject,
        PubResolvedGraph,
    };
    use pub_model::{
        Document, DocumentId, LengthEmu, Page, Sha256Digest, Size2D, SourceDescriptor,
    };

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x5a; 32])
    }

    fn authored_page_id() -> PageId {
        serde_json::from_str("\"01890f4f-1234-7abc-8def-0123456789ab\"")
            .expect("valid UUIDv7 PageId")
    }

    fn source_graph(source_pages: Vec<PageId>) -> PubResolvedGraph {
        let hash = source_hash();
        let pages = source_pages
            .iter()
            .copied()
            .map(|page_id| {
                (
                    page_id,
                    Page {
                        id: page_id,
                        size: Size2D::new(LengthEmu::new(914_400), LengthEmu::new(914_400)),
                        bleed: None,
                        margins: None,
                        children: Vec::new(),
                        extensions: Vec::new(),
                    },
                )
            })
            .collect();

        PubResolvedGraph {
            cdm_version: "0.1".into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-rs/test".into(),
                source_hash: hash,
            },
            document: Document {
                id: serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                    .expect("document id"),
                format_origin: "pub".into(),
                source_hash: hash,
                pages: source_pages,
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages,
            nodes: BTreeMap::new(),
            stories: BTreeMap::new(),
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    fn identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: authored_page_id(),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    #[test]
    fn registered_page_identity_is_history_derived_and_source_graph_immutable() {
        let graph = source_graph(Vec::new());
        let source_before = graph.clone();
        let mut session = EditorSession::new(graph).expect("session");

        let operation = session
            .register_authored_page_identity_v1(identity())
            .expect("register authored Page identity");

        assert!(matches!(
            operation,
            EditOperation::RegisterAuthoredPageIdentityV1 { .. }
        ));
        assert_eq!(session.graph(), &source_before);
        assert_eq!(
            session.authored_page_identities_v1().get(&authored_page_id()),
            Some(&identity())
        );

        session.undo().expect("undo identity registration");
        assert!(session.authored_page_identities_v1().is_empty());
        assert_eq!(session.graph(), &source_before);

        session.redo().expect("redo identity registration");
        assert_eq!(
            session.authored_page_identities_v1().get(&authored_page_id()),
            Some(&identity())
        );
        assert_eq!(session.graph(), &source_before);
    }

    #[test]
    fn authored_page_identity_project_replays_exact_id_and_provenance() {
        let graph = source_graph(Vec::new());
        let mut session = EditorSession::new(graph.clone()).expect("session");
        session
            .register_authored_page_identity_v1(identity())
            .expect("register identity");

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_24);
        let encoded = serde_json::to_vec(&project).expect("serialize project");
        let decoded: EditorProject = serde_json::from_slice(&encoded).expect("deserialize project");

        let mut reopened = EditorSession::new(graph).expect("fresh session");
        reopened.apply_project(&decoded).expect("replay identity");

        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(
            reopened.authored_page_identities_v1().get(&authored_page_id()),
            Some(&identity())
        );
    }

    #[test]
    fn authored_page_identity_rejects_source_page_collision() {
        let page_id = authored_page_id();
        let mut session = EditorSession::new(source_graph(vec![page_id])).expect("session");

        assert_eq!(
            session.register_authored_page_identity_v1(identity()),
            Err(EditorError::AuthoredPageIdentityConflict { page_id })
        );
        assert!(session.operations().is_empty());
    }

    #[test]
    fn authored_page_identity_rejects_duplicate_history_identity() {
        let page_id = authored_page_id();
        let mut session = EditorSession::new(source_graph(Vec::new())).expect("session");
        session
            .register_authored_page_identity_v1(identity())
            .expect("first registration");

        assert_eq!(
            session.register_authored_page_identity_v1(identity()),
            Err(EditorError::AuthoredPageIdentityConflict { page_id })
        );
        assert_eq!(session.operations().len(), 1);
    }
}
