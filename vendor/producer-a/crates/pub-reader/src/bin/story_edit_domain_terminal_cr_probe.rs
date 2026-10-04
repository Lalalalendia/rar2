use pub_model::Sha256Digest;
use pub_reader::build_mature_0x2c_source_graph;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, io::Cursor, path::PathBuf};

const SCHEMA: &str = "chaptera.story-edit-domain-terminal-cr-probe.v1";

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = PathBuf::from(
        env::args()
            .nth(1)
            .ok_or("usage: story_edit_domain_terminal_cr_probe INPUT")?,
    );
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);

    let build = match build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash) {
        Ok(build) => build,
        Err(_) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": SCHEMA,
                    "source_sha256": hash.to_string(),
                    "source_bytes": bytes.len(),
                    "open_state": "not_admitted",
                    "story_count": 0,
                    "stories": [],
                    "claims": {
                        "measurement_only": true,
                        "terminal_cr_provenance_promoted": false,
                        "story_text_recorded": false
                    }
                }))?
            );
            return Ok(());
        }
    };

    let stories = build
        .graph
        .stories
        .values()
        .map(|story| {
            let scalar_count = story.text.chars().count();
            let utf16_count = story.text.encode_utf16().count();
            let terminal_cr = story.text.ends_with('\r');
            let cr_only = story.text == "\r";
            let empty = story.text.is_empty();
            let has_fdpp_story_end = story
                .source_refs
                .iter()
                .any(|source_ref| source_ref.path.as_deref() == Some("FDPP/storyEnd"));
            let has_story_catalog_identity = story.source_refs.iter().any(|source_ref| {
                source_ref.path.as_deref() == Some("Contents/0x65/textId")
            });
            let has_direct_quill_syid = story
                .source_refs
                .iter()
                .any(|source_ref| source_ref.path.as_deref() == Some("SYID"));
            let exact_text_ref = story.source_refs.iter().any(|source_ref| {
                source_ref.path.as_deref() == Some("TEXT")
                    && source_ref.confidence == Some(pub_model::ReadConfidence::Exact)
            });

            json!({
                "story_id": story.id.as_canonical().to_string(),
                "scalar_count": scalar_count,
                "utf16_count": utf16_count,
                "terminal_cr": terminal_cr,
                "cr_only": cr_only,
                "empty": empty,
                "has_fdpp_story_end": has_fdpp_story_end,
                "has_story_catalog_identity": has_story_catalog_identity,
                "has_direct_quill_syid": has_direct_quill_syid,
                "exact_text_ref": exact_text_ref,
                "explicit_fdpp_terminal_cr_candidate": (
                    has_fdpp_story_end
                    && has_story_catalog_identity
                    && exact_text_ref
                    && terminal_cr
                )
            })
        })
        .collect::<Vec<_>>();

    let count = |key: &str| -> usize {
        stories
            .iter()
            .filter(|story| story.get(key).and_then(|value| value.as_bool()) == Some(true))
            .count()
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": SCHEMA,
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "open_state": "admitted",
            "story_count": stories.len(),
            "fdpp_bounded_story_count": count("has_fdpp_story_end"),
            "story_catalog_identity_count": count("has_story_catalog_identity"),
            "direct_quill_syid_story_count": count("has_direct_quill_syid"),
            "exact_text_ref_story_count": count("exact_text_ref"),
            "terminal_cr_story_count": count("terminal_cr"),
            "cr_only_story_count": count("cr_only"),
            "empty_story_count": count("empty"),
            "explicit_fdpp_terminal_cr_candidate_count": count(
                "explicit_fdpp_terminal_cr_candidate"
            ),
            "stories": stories,
            "claims": {
                "measurement_only": true,
                "terminal_cr_provenance_promoted": false,
                "story_text_recorded": false
            }
        }))?
    );
    Ok(())
}
