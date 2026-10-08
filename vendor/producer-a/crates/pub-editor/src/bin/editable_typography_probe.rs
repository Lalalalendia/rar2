use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

const SCHEMA: &str = "chaptera.editable-full-story-typography-probe.v1";
const EMU_PER_POINT: f64 = 12_700.0;

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
            .ok_or("usage: editable_typography_probe INPUT")?,
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
                    "items": [],
                }))?
            );
            return Ok(());
        }
    };

    let typography = session.full_story_typography_v1();
    let items = typography
        .iter()
        .map(|item| {
            serde_json::json!({
                "story_id": item.story_id.as_canonical().to_string(),
                "font_family": item.font_family,
                "font_size_emu": item.font_size_emu.get(),
                "font_size_pt": item.font_size_emu.get() as f64 / EMU_PER_POINT,
            })
        })
        .collect::<Vec<serde_json::Value>>();

    let mut same_length_edit_invalidation_proven = None;
    if let Some(item) = typography.first() {
        let story_id = item.story_id;
        let before = session
            .graph()
            .stories
            .get(&story_id)
            .ok_or("eligible typography Story disappeared")?
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
            return Err("could not construct same-length typography invalidation edit".into());
        }

        session.replace_story_text(story_id, replacement)?;
        if session
            .full_story_typography_v1()
            .iter()
            .any(|candidate| candidate.story_id == story_id)
        {
            return Err("source typography survived a same-length Story edit".into());
        }

        session.undo()?;
        if !session
            .full_story_typography_v1()
            .iter()
            .any(|candidate| candidate == item)
        {
            return Err("source typography did not return after exact Story undo".into());
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
            "eligible_count": items.len(),
            "same_length_edit_invalidation_proven": same_length_edit_invalidation_proven,
            "items": items,
        }))?
    );
    Ok(())
}
