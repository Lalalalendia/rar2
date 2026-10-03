use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

const SCHEMA: &str = "chaptera.editable-full-story-typography-probe.v1";
const EMU_PER_POINT: f64 = 12_700.0;

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
            .ok_or("usage: editable_typography_probe INPUT")?,
    );
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);

    let session = match open_mature_0x2c_editor(&bytes, hash) {
        Ok(session) => session,
        Err(_) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
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

    let items = session
        .full_story_typography_v1()
        .into_iter()
        .map(|item| {
            json!({
                "story_id": item.story_id.as_canonical().to_string(),
                "font_family": item.font_family,
                "font_size_emu": item.font_size_emu.get(),
                "font_size_pt": item.font_size_emu.get() as f64 / EMU_PER_POINT,
            })
        })
        .collect::<Vec<Value>>();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": SCHEMA,
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "open_state": "admitted",
            "eligible_count": items.len(),
            "items": items,
        }))?
    );
    Ok(())
}
