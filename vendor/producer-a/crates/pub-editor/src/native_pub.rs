//! Product-level native PUB materialization gate for Editor final state.
//!
//! This module deliberately admits only the smallest writer surface already
//! proven end-to-end: one effective ordinary Story text mutation on a mature
//! 0x2C source. Any additional effective persistence requirement keeps the
//! project in Project-only mode.

use super::{EditorPubWriterAssessmentError, EditorSession, EffectiveStoryTextMutation};
use pub_export::PersistenceRequirement;
use pub_model::{Sha256Digest, StoryId};
use pub_writer::{
    StoryTextPubMaterializationBlocked, StoryTextWriteProbeRequest,
    materialize_mature_0x2c_story_text_pub_candidate,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorNativePubCandidate {
    pub source_hash: Sha256Digest,
    pub output_hash: Sha256Digest,
    pub source_story_id: StoryId,
    pub output_story_id: StoryId,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorNativePubMaterializationBlocked {
    Assessment(EditorPubWriterAssessmentError),
    EffectiveStoryMutationCount {
        found: usize,
    },
    UnsupportedEffectiveState {
        requirements: Vec<PersistenceRequirement>,
    },
    Writer(StoryTextPubMaterializationBlocked),
}

impl EditorNativePubMaterializationBlocked {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Assessment(_) => "editor_pub_assessment",
            Self::EffectiveStoryMutationCount { .. } => "editor_pub_story_mutation_count",
            Self::UnsupportedEffectiveState { .. } => "editor_pub_effective_state_unsupported",
            Self::Writer(error) => error.code(),
        }
    }
}

impl fmt::Display for EditorNativePubMaterializationBlocked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assessment(error) => write!(formatter, "PUB writer assessment failed: {error}"),
            Self::EffectiveStoryMutationCount { found } => write!(
                formatter,
                "native PUB save requires exactly one effective ordinary Story mutation; found {found}"
            ),
            Self::UnsupportedEffectiveState { requirements } => write!(
                formatter,
                "native PUB save is blocked because the final Editor state requires {} persistence capabilities outside the single-Story text slice",
                requirements.len()
            ),
            Self::Writer(error) => write!(
                formatter,
                "native PUB writer blocked the final mutation: {error}"
            ),
        }
    }
}

impl std::error::Error for EditorNativePubMaterializationBlocked {}

impl From<EditorPubWriterAssessmentError> for EditorNativePubMaterializationBlocked {
    fn from(value: EditorPubWriterAssessmentError) -> Self {
        Self::Assessment(value)
    }
}

impl From<StoryTextPubMaterializationBlocked> for EditorNativePubMaterializationBlocked {
    fn from(value: StoryTextPubMaterializationBlocked) -> Self {
        Self::Writer(value)
    }
}

impl EditorSession {
    /// Materialize the current Editor final state into a whole-file PUB candidate
    /// only when the state is exactly one proven ordinary Story text mutation.
    ///
    /// The immutable source bytes are never modified. The delegated writer
    /// replaces only the bounded Quill stream in a copied CFB, then reopens the
    /// whole candidate through the current Reader and verifies the target Story.
    ///
    /// Success here proves Chaptera round-trip acceptance. Native Microsoft
    /// Publisher acceptance remains a separate validation gate.
    pub fn materialize_mature_0x2c_native_pub_candidate(
        &self,
        source_pub: &[u8],
    ) -> Result<EditorNativePubCandidate, EditorNativePubMaterializationBlocked> {
        let mutations = self.effective_ordinary_story_text_mutations()?;
        if mutations.len() != 1 {
            return Err(
                EditorNativePubMaterializationBlocked::EffectiveStoryMutationCount {
                    found: mutations.len(),
                },
            );
        }

        let requirements = self.effective_pub_persistence_requirements()?;
        let expected = vec![story_text_requirement(mutations[0].story_id)];
        if requirements != expected {
            return Err(
                EditorNativePubMaterializationBlocked::UnsupportedEffectiveState { requirements },
            );
        }

        let EffectiveStoryTextMutation {
            story_id,
            before,
            after,
        } = mutations.into_iter().next().expect("exactly one mutation");

        let request = StoryTextWriteProbeRequest {
            source_hash: self.source_hash(),
            story_id,
            before,
            after,
        };
        let candidate = materialize_mature_0x2c_story_text_pub_candidate(source_pub, &request)?;

        Ok(EditorNativePubCandidate {
            source_hash: candidate.source_hash,
            output_hash: candidate.output_hash,
            source_story_id: candidate.source_story_id,
            output_story_id: candidate.output_story_id,
            bytes: candidate.bytes,
        })
    }
}

fn story_text_requirement(story_id: StoryId) -> PersistenceRequirement {
    PersistenceRequirement {
        feature: "story.text".into(),
        origin: Some(story_id.into_canonical()),
        property_path: Some("story.text".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LengthEmu, open_mature_0x2c_editor};
    use pub_reader::derive_pub_story_id;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn sample3_pub() -> Vec<u8> {
        decode_base64(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../pub-quill/tests/fixtures/Sample3.pub.b64"
        )))
    }

    fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
        let digest = Sha256::digest(bytes);
        let mut value = [0_u8; 32];
        value.copy_from_slice(&digest);
        Sha256Digest::from_bytes(value)
    }

    fn controlled_story(session: &EditorSession, source_hash: Sha256Digest) -> (StoryId, String) {
        let story_id = derive_pub_story_id(&source_hash, 4).expect("source StoryId");
        let text = session.graph().stories.get(&story_id).expect("controlled Story").text.clone();
        (story_id, text)
    }

    #[test]
    fn single_effective_story_mutation_materializes_reopenable_pub() {
        let source = sample3_pub();
        let source_before = source.clone();
        let source_hash = sha256_digest(&source);
        let mut session =
            open_mature_0x2c_editor(&source, source_hash).expect("open bounded editor");
        let (story_id, before) = controlled_story(&session, source_hash);
        assert!(before.contains("345678"), "controlled marker must exist");
        let after = before.replacen("345678", "", 1);
        session
            .replace_story_text(story_id, after.clone())
            .expect("ordinary Story edit");

        let candidate = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect("native PUB candidate");

        assert_eq!(source, source_before, "source bytes are immutable");
        assert_eq!(candidate.source_hash, source_hash);
        assert_ne!(candidate.output_hash, source_hash);
        assert_eq!(candidate.source_story_id, story_id);

        let reopened = pub_reader::build_mature_0x2c_source_graph(
            Cursor::new(&candidate.bytes),
            candidate.output_hash,
        )
        .expect("candidate SourceGraph");
        let resolved =
            pub_reader::resolve_pub_source_graph(&reopened.graph).expect("candidate resolve");
        assert_eq!(
            resolved.graph.stories[&candidate.output_story_id].text,
            after
        );
    }

    #[test]
    fn no_effective_story_mutation_is_never_a_native_pub_save() {
        let source = sample3_pub();
        let source_hash = sha256_digest(&source);
        let session = open_mature_0x2c_editor(&source, source_hash).expect("bounded Editor");
        let error = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect_err("no-op source remains Project-only");
        assert_eq!(error.code(), "editor_pub_story_mutation_count");
    }

    #[test]
    fn additional_geometry_requirement_blocks_native_pub_save() {
        let source = sample3_pub();
        let source_hash = sha256_digest(&source);
        let mut session =
            open_mature_0x2c_editor(&source, source_hash).expect("open bounded editor");
        let (story_id, before) = controlled_story(&session, source_hash);
        let after = before.replacen("345678", "", 1);
        session
            .replace_story_text(story_id, after)
            .expect("ordinary Story edit");

        let node_id = session
            .graph()
            .nodes
            .iter()
            .find_map(|(node_id, node)| {
                let next_x = LengthEmu::new(node.header.bounds.x.get().checked_add(1)?);
                session
                    .can_move_node_to(*node_id, next_x, node.header.bounds.y)
                    .ok()
                    .map(|_| *node_id)
            })
            .expect("movable geometry candidate");
        let before_bounds = session.graph().nodes[&node_id].header.bounds;
        session
            .move_node_to(
                node_id,
                LengthEmu::new(before_bounds.x.get() + 1),
                before_bounds.y,
            )
            .expect("bounded move");

        let error = session
            .materialize_mature_0x2c_native_pub_candidate(&source)
            .expect_err("mixed final state must remain Project-only");
        assert_eq!(error.code(), "editor_pub_effective_state_unsupported");
    }

    fn decode_base64(text: &str) -> Vec<u8> {
        let cleaned = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        assert_eq!(cleaned.len() % 4, 0, "base64 fixture length");

        let mut output = Vec::with_capacity(cleaned.len() / 4 * 3);
        for quartet in cleaned.chunks_exact(4) {
            let a = base64_value(quartet[0]);
            let b = base64_value(quartet[1]);
            let c = if quartet[2] == b'=' {
                0
            } else {
                base64_value(quartet[2])
            };
            let d = if quartet[3] == b'=' {
                0
            } else {
                base64_value(quartet[3])
            };
            output.push((a << 2) | (b >> 4));
            if quartet[2] != b'=' {
                output.push((b << 4) | (c >> 2));
            }
            if quartet[3] != b'=' {
                output.push((c << 6) | d);
            }
        }
        output
    }

    fn base64_value(byte: u8) -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("invalid base64 byte {other:#x}"),
        }
    }
}
