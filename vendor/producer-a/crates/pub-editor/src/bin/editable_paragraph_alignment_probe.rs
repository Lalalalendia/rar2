use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

const SCHEMA: &str = "chaptera.editable-full-story-paragraph-alignment-probe.v1";

fn source_hash(bytes: &[u8]) -> pub_editor::Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    pub_editor::Sha256Digest::from_bytes(raw)
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = PathBuf::from(
        env::args()
            .nth(1)
            .ok_or("usage: editable_paragraph_alignment_probe INPUT")?,
    );
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);

    let mut session = match pub_editor::open_mature_0x2c_editor(&bytes, hash) {
        Ok(session) => session,
        Err(_) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema": SCHEMA,
                    "source_sha256": hash.to_string(),
                    "source_bytes": bytes.len(),
                    "open_state": "not_admitted",
                    "eligible_count": 0,
                    "alignment_counts": {},
                    "items": [],
                }))?
            );
            return Ok(());
        }
    };

    let alignments = session.full_story_paragraph_alignment_v1();
    let mut center = 0_u64;
    let mut right = 0_u64;
    let items = alignments
        .iter()
        .map(|item| {
            let alignment = match item.alignment {
                pub_export::ParagraphAlignmentV1::Center => {
                    center += 1;
                    "center"
                }
                pub_export::ParagraphAlignmentV1::Right => {
                    right += 1;
                    "right"
                }
            };
            let story = session
                .graph()
                .stories
                .get(&item.story_id)
                .expect("eligible paragraph-alignment Story must exist");
            let non_whitespace = story
                .text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            let non_whitespace_sha256 = format!("{:x}", Sha256::digest(non_whitespace.as_bytes()));
            serde_json::json!({
                "story_id": item.story_id.as_canonical().to_string(),
                "alignment": alignment,
                "story_scalar_count": story.text.chars().count(),
                "story_non_whitespace_scalar_count": non_whitespace.chars().count(),
                "story_non_whitespace_sha256": non_whitespace_sha256,
            })
        })
        .collect::<Vec<_>>();

    let mut same_length_edit_invalidation_proven = None;
    if let Some(item) = alignments.first() {
        let story_id = item.story_id;
        let before = session
            .graph()
            .stories
            .get(&story_id)
            .ok_or("eligible paragraph-alignment Story disappeared")?
            .text
            .clone();
        let replacement = before
            .chars()
            .map(|character| {
                if character.len_utf16() == 2 {
                    match character {
                        '😀' => '😃',
                        _ => '😀',
                    }
                } else if character == 'x' {
                    'y'
                } else {
                    'x'
                }
            })
            .collect::<String>();
        if replacement == before
            || replacement.chars().count() != before.chars().count()
            || replacement.encode_utf16().count() != before.encode_utf16().count()
        {
            return Err(
                "could not construct same-length paragraph-alignment invalidation edit".into(),
            );
        }

        session.replace_story_text(story_id, replacement)?;
        if session
            .full_story_paragraph_alignment_v1()
            .iter()
            .any(|candidate| candidate.story_id == story_id)
        {
            return Err("source paragraph alignment survived a same-length Story edit".into());
        }

        session.undo()?;
        if !session
            .full_story_paragraph_alignment_v1()
            .iter()
            .any(|candidate| candidate == item)
        {
            return Err("source paragraph alignment did not return after exact Story undo".into());
        }
        same_length_edit_invalidation_proven = Some(true);
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": SCHEMA,
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "open_state": "admitted",
            "eligible_count": alignments.len(),
            "alignment_counts": {
                "center": center,
                "right": right,
            },
            "items": items,
            "same_length_edit_invalidation_proven": same_length_edit_invalidation_proven,
        }))?
    );
    Ok(())
}
