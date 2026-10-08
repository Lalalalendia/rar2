use super::*;

pub(super) fn editor_project_asset_metadata(asset: &EditorReplacementAsset) -> EditorProjectAsset {
    EditorProjectAsset {
        sha256: asset.sha256,
        mime: asset.mime.clone(),
        byte_len: u64::try_from(asset.bytes.len())
            .expect("validated editor asset length must fit u64"),
    }
}

pub(super) fn canonical_editor_asset_metadata(
    assets: &BTreeMap<Sha256Digest, EditorReplacementAsset>,
) -> Vec<EditorProjectAsset> {
    assets.values().map(editor_project_asset_metadata).collect()
}

pub fn editor_asset_file_name(
    sha256: Sha256Digest,
    mime: &str,
) -> Result<String, EditorAssetError> {
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        other => {
            return Err(EditorAssetError::UnsupportedMime {
                mime: other.to_owned(),
            });
        }
    };
    Ok(format!("asset-{sha256}.{extension}"))
}

pub(super) fn validated_editor_asset(
    mime: String,
    bytes: Vec<u8>,
) -> Result<EditorReplacementAsset, EditorAssetError> {
    if bytes.is_empty() {
        return Err(EditorAssetError::EmptyBytes);
    }
    u64::try_from(bytes.len()).map_err(|_| EditorAssetError::ByteLengthOverflow)?;

    let signature_matches = match mime.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        other => {
            return Err(EditorAssetError::UnsupportedMime {
                mime: other.to_owned(),
            });
        }
    };
    if !signature_matches {
        return Err(EditorAssetError::SignatureMismatch { mime });
    }

    let digest = Sha256::digest(&bytes);
    let mut digest_bytes = [0_u8; 32];
    digest_bytes.copy_from_slice(&digest);
    Ok(EditorReplacementAsset {
        sha256: Sha256Digest::from_bytes(digest_bytes),
        mime,
        bytes,
    })
}
