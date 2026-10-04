use pub_editor::{EditorProject, Sha256Digest, open_mature_0x2c_editor};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    path::PathBuf,
};

fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

#[derive(Debug)]
struct Args {
    source: PathBuf,
    project: PathBuf,
    assets: Vec<PathBuf>,
    output: PathBuf,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut values = env::args().skip(1);
    let mut source = None;
    let mut project = None;
    let mut assets = Vec::new();
    let mut output = None;

    while let Some(arg) = values.next() {
        match arg.as_str() {
            "--source" => {
                source = Some(PathBuf::from(
                    values.next().ok_or("--source requires a path")?,
                ))
            }
            "--project" => {
                project = Some(PathBuf::from(
                    values.next().ok_or("--project requires a path")?,
                ))
            }
            "--asset" => assets.push(PathBuf::from(
                values.next().ok_or("--asset requires a path")?,
            )),
            "--output" => {
                output = Some(PathBuf::from(
                    values.next().ok_or("--output requires a path")?,
                ))
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok(Args {
        source: source.ok_or("--source is required")?,
        project: project.ok_or("--project is required")?,
        assets,
        output: output.ok_or("--output is required")?,
    })
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
    let source_hash = sha256_digest(&source_bytes);

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
            Ok::<_, std::io::Error>((sha256_digest(&bytes), bytes))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let asset_bytes = exact_asset_map(&project, asset_inputs)?;

    let mut session = open_mature_0x2c_editor(&source_bytes, source_hash)?;
    session.apply_project_with_assets(&project, &asset_bytes)?;
    let state = session.fixed_output_state_v1()?;

    if let Some(parent) = args
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_vec_pretty(&state)?)?;

    println!(
        "{}",
        serde_json::json!({
            "status": "valid",
            "schema_version": state.schema_version,
            "source_hash": state.source_hash.to_string(),
            "project_state_id": state.project_state_id,
            "image_resource_count": state.image_resources.len(),
            "output": args.output,
            "invariants": {
                "authoritative_rust_project_replay": true,
                "source_reparse_after_project_apply_count": 0,
                "renderer_safe": false
            }
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::EditorProjectAsset;

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::from_bytes([byte; 32])
    }

    fn project(assets: Vec<EditorProjectAsset>) -> EditorProject {
        EditorProject {
            schema_version: "pub-editor-v0.11".into(),
            source_hash: digest(0x44),
            identity: None,
            assets,
            table_grids: Vec::new(),
            operations: Vec::new(),
        }
    }

    #[test]
    fn asset_inputs_must_match_project_identity_set_exactly() {
        let a = digest(0x11);
        let b = digest(0x22);
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
