use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn crop_json(crop: pub_editor::ImageCropStateV1) -> Value {
    json!({
        "top_raw": crop.top_raw,
        "bottom_raw": crop.bottom_raw,
        "left_raw": crop.left_raw,
        "right_raw": crop.right_raw,
    })
}

fn crop_nonzero(crop: pub_editor::ImageCropStateV1) -> bool {
    [crop.top_raw, crop.bottom_raw, crop.left_raw, crop.right_raw]
        .into_iter()
        .flatten()
        .any(|value| value != 0)
}

fn inspect(path: &Path) -> Value {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return json!({
                "file": path.file_name().and_then(|v| v.to_str()).unwrap_or("<unknown>"),
                "status": "read_error",
                "error": error.to_string(),
            });
        }
    };
    let hash = source_hash(&bytes);
    let mut editor = match open_mature_0x2c_editor(&bytes, hash) {
        Ok(editor) => editor,
        Err(error) => {
            return json!({
                "file": path.file_name().and_then(|v| v.to_str()).unwrap_or("<unknown>"),
                "status": "open_unsupported",
                "sha256": hash.to_string(),
                "byte_len": bytes.len(),
                "error": error.to_string(),
            });
        }
    };

    let replacement = editor
        .import_replacement_asset("image/png", b"\x89PNG\r\n\x1a\nchaptera-crop-census".to_vec())
        .expect("census PNG signature must be accepted");

    let node_ids = editor.graph().nodes.keys().copied().collect::<Vec<_>>();
    let mut crops = Vec::new();
    let mut nonzero_count = 0_usize;
    let mut replace_eligible_nonzero_count = 0_usize;
    for node_id in node_ids {
        let Some(crop) = editor.image_crop_for(node_id) else {
            continue;
        };
        let nonzero = crop_nonzero(crop);
        let replace_eligible = editor.can_replace_image(node_id, replacement).is_ok();
        if nonzero {
            nonzero_count += 1;
            if replace_eligible {
                replace_eligible_nonzero_count += 1;
            }
        }
        crops.push(json!({
            "node_id": node_id.as_canonical().to_string(),
            "crop": crop_json(crop),
            "nonzero": nonzero,
            "replace_eligible": replace_eligible,
        }));
    }

    json!({
        "file": path.file_name().and_then(|v| v.to_str()).unwrap_or("<unknown>"),
        "status": "opened",
        "sha256": hash.to_string(),
        "byte_len": bytes.len(),
        "explicit_crop_count": crops.len(),
        "nonzero_crop_count": nonzero_count,
        "replace_eligible_nonzero_crop_count": replace_eligible_nonzero_count,
        "crops": crops,
    })
}

fn main() {
    let paths = env::args().skip(1).collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("usage: crop_census <file.pub>...");
        std::process::exit(2);
    }
    let results = paths
        .iter()
        .map(|path| inspect(Path::new(path)))
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&results).expect("serialize census"));
}
