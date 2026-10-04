use chaptera_desktop_shaped_flow_runtime::{
    ExplicitDesktopFontResourceV1, build_current_fixed_output_packet_v1,
};
use pub_editor::{EditorProject, Sha256Digest, open_mature_0x2c_editor};
use pub_layout::font_fingerprint_sha256;
use pub_model::{EMU_PER_POINT, LengthEmu};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    path::PathBuf,
};

const FONT_SIZE_PT: i64 = 8;
const LINE_HEIGHT_MULTIPLIER: i64 = 2;

#[derive(Debug)]
struct Args {
    source: PathBuf,
    project: PathBuf,
    assets: Vec<PathBuf>,
    font: PathBuf,
    output: PathBuf,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut values = env::args().skip(1);
    let mut source = None;
    let mut project = None;
    let mut assets = Vec::new();
    let mut font = None;
    let mut output = None;

    while let Some(arg) = values.next() {
        match arg.as_str() {
            "--source" => source = Some(PathBuf::from(values.next().ok_or("--source requires a path")?)),
            "--project" => {
                project = Some(PathBuf::from(values.next().ok_or("--project requires a path")?))
            }
            "--asset" => assets.push(PathBuf::from(values.next().ok_or("--asset requires a path")?)),
            "--font" => font = Some(PathBuf::from(values.next().ok_or("--font requires a path")?)),
            "--output" => output = Some(PathBuf::from(values.next().ok_or("--output requires a path")?)),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok(Args {
        source: source.ok_or("--source is required")?,
        project: project.ok_or("--project is required")?,
        assets,
        font: font.ok_or("--font is required")?,
        output: output.ok_or("--output is required")?,
    })
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn exact_asset_map(
    project: &EditorProject,
    asset_inputs: impl IntoIterator<Item = (Sha256Digest, Vec<u8>)>,
) -> Result<BTreeMap<Sha256Digest, Vec<u8>>, Box<dyn Error>> {
    let expected = project
        .assets
        .iter()
        .map(|asset| asset.sha256)
        .collect::<BTreeSet<_>>();
    let mut supplied = BTreeMap::new();

    for (sha256, bytes) in asset_inputs {
        if supplied.insert(sha256, bytes).is_some() {
            return Err(format!("duplicate asset input for {sha256}").into());
        }
    }

    let found = supplied.keys().copied().collect::<BTreeSet<_>>();
    if found != expected {
        return Err(format!(
            "asset input identity set mismatch: expected={} found={}",
            expected.len(),
            found.len()
        )
        .into());
    }
    Ok(supplied)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let source_bytes = fs::read(&args.source)?;
    let source_hash = digest(&source_bytes);

    let project_bytes = fs::read(&args.project)?;
    let project: EditorProject = serde_json::from_slice(&project_bytes)?;
    if project.source_hash != source_hash {
        return Err("EditorProject source identity differs from immutable PUB bytes".into());
    }

    let asset_inputs = args
        .assets
        .iter()
        .map(|path| {
            let bytes = fs::read(path)?;
            Ok::<_, std::io::Error>((digest(&bytes), bytes))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let asset_bytes = exact_asset_map(&project, asset_inputs)?;

    let font_bytes = fs::read(&args.font)?;
    let font_fingerprint = font_fingerprint_sha256(&font_bytes);
    let font_size_emu = LengthEmu::new(FONT_SIZE_PT * EMU_PER_POINT);
    let font = ExplicitDesktopFontResourceV1 {
        resource_id: "chaptera:pinned-fallback-font",
        expected_sha256: &font_fingerprint,
        face_index: 0,
        font_size_emu,
        line_height_emu: LengthEmu::new(
            font_size_emu
                .get()
                .checked_mul(LINE_HEIGHT_MULTIPLIER)
                .ok_or("fixed-output line-height overflow")?,
        ),
        bytes: &font_bytes,
    };

    let mut editor = open_mature_0x2c_editor(&source_bytes, source_hash)?;
    editor.apply_project_with_assets(&project, &asset_bytes)?;
    let packet = build_current_fixed_output_packet_v1(&editor, &font)?;

    if packet.source_hash != source_hash.to_string() {
        return Err("fixed-output packet source identity mismatch".into());
    }
    if packet.project_state_id != project.state_id_v1() {
        return Err("fixed-output packet project state identity mismatch".into());
    }
    if packet.invariants.source_reparse_after_project_apply_count != 0 {
        return Err("fixed-output packet reports a post-edit source reparse".into());
    }
    if packet.invariants.source_refs_in_renderer_packet {
        return Err("fixed-output packet reports source provenance leakage".into());
    }

    if let Some(parent) = args.output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_vec_pretty(&packet)?;
    fs::write(&args.output, &encoded)?;

    println!(
        "{}",
        serde_json::json!({
            "status": "valid",
            "source_sha256": source_hash.to_string(),
            "project_file_sha256": hex_sha256(&project_bytes),
            "project_state_id": packet.project_state_id,
            "packet_sha256": hex_sha256(&encoded),
            "story_state_count": packet.story_states.len(),
            "line_count": packet.shaped_flow.lines.len(),
            "node_paint_count": packet.node_paints.len(),
            "image_resource_count": packet.image_resources.len(),
            "font_fingerprint_sha256": packet.font.fingerprint_sha256,
            "output": args.output,
            "invariants": packet.invariants,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::EditorProjectAsset;

    fn test_digest(byte: u8) -> Sha256Digest {
        Sha256Digest::from_bytes([byte; 32])
    }

    fn project(assets: Vec<EditorProjectAsset>) -> EditorProject {
        EditorProject {
            schema_version: "pub-editor-v0.11".into(),
            source_hash: test_digest(0x44),
            identity: None,
            assets,
            table_grids: Vec::new(),
            operations: Vec::new(),
        }
    }

    #[test]
    fn packet_asset_inputs_must_match_project_assets_exactly() {
        let a = test_digest(0x11);
        let b = test_digest(0x22);
        let project = project(vec![EditorProjectAsset {
            sha256: a,
            mime: "image/png".into(),
            byte_len: 3,
        }]);

        assert!(exact_asset_map(&project, [(a, vec![1, 2, 3])]).is_ok());
        assert!(exact_asset_map(&project, [(a, vec![1, 2, 3]), (b, vec![4])]).is_err());
        assert!(exact_asset_map(&project, std::iter::empty()).is_err());
    }
}
