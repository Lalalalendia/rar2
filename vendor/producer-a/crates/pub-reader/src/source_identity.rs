//! Canonical source identity and provenance helpers for mature PUB.
//! CI leaf-control: comment-only source identity steady-state proof.
//!
//! This owner defines stable object-key vocabularies, source-derived model IDs,
//! and exact byte-range SourceRef construction. Parsing, Story decoding,
//! publication policy and graph assembly remain in their existing owners.

use anyhow::{Result, anyhow};
use pub_core::RawSpan;
use pub_model::{
    AuthorityClass, ByteRange, CanonicalId, DocumentId, NodeId, PageId, ReadConfidence,
    Sha256Digest, SourceDerivedIdInput, SourceDescriptor, SourceRef, SourceRole, StoryId,
    derive_source_canonical_id,
};

use super::PUB_ADAPTER_ID;

pub(super) const ROLE_DOCUMENT: &str = "cdm.document";
pub(super) const ROLE_PAGE: &str = "cdm.page";
pub(super) const ROLE_NODE: &str = "cdm.node";
pub(super) const ROLE_STORY: &str = "cdm.story";

/// Canonical source key for a physical mature-0x2C Contents directory slot.
pub fn contents_object_key(seq_num: u32) -> String {
    format!("contents/0x2c/seq/{seq_num}")
}

/// Canonical source key for a persistent Quill SYID story identity.
pub fn quill_story_object_key(syid: u32) -> String {
    format!("quill/syid/{syid}")
}

pub fn derive_pub_document_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<DocumentId> {
    Ok(DocumentId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_DOCUMENT,
    )?))
}

pub fn derive_pub_page_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<PageId> {
    Ok(PageId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_PAGE,
    )?))
}

pub fn derive_pub_node_id(source_hash: &Sha256Digest, seq_num: u32) -> Result<NodeId> {
    Ok(NodeId::from_canonical(derive_pub_id(
        source_hash,
        &contents_object_key(seq_num),
        ROLE_NODE,
    )?))
}

pub fn derive_pub_story_id(source_hash: &Sha256Digest, syid: u32) -> Result<StoryId> {
    Ok(StoryId::from_canonical(derive_pub_id(
        source_hash,
        &quill_story_object_key(syid),
        ROLE_STORY,
    )?))
}

pub(super) fn derive_pub_id(
    source_hash: &Sha256Digest,
    object_key: &str,
    semantic_role: &str,
) -> Result<CanonicalId> {
    derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: PUB_ADAPTER_ID,
        source_object_key: object_key,
        semantic_role,
    })
    .map_err(|error| anyhow!("source-derived identity error: {error:?}"))
}

pub(super) fn source_ref(
    source: &SourceDescriptor,
    span: &RawSpan,
    object_key: Option<String>,
    path: Option<String>,
    role: SourceRole,
    authority: AuthorityClass,
    confidence: ReadConfidence,
) -> SourceRef {
    SourceRef {
        format: source.format.clone(),
        adapter_version: source.adapter_version.clone(),
        source_hash: source.source_hash,
        carrier: span.stream.0.clone(),
        object_key,
        path,
        byte_range: Some(ByteRange::new(span.offset, span.len)),
        role,
        authority,
        confidence: Some(confidence),
    }
}
