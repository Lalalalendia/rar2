//! Pinned full-font Editor command bridge, not native PUB Save or layout.
 //! Host supplies current Scene scope independently of the browser intent.
 //! This CLI never fetches fonts by name, path, URL, or a client descriptor.
use anyhow::{Context, Result, ensure};
use chaptera_text_format_overlay::{
    FontAuthoringScopeV1, FontReplacementCandidateV1, FontResourceIdentityV1,
    ServerFontResourceV1,
};
use pub_editor::{
    EditorProject, EditorProjectFontReopenGrantV1, EditorSession, Sha256Digest, StoryId,
    open_mature_0x2c_editor,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

const SOURCE_SHA: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
const FONT_SHA: &str = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4";
const FONT_ID: &str = "f27a8036-8492-480f-8fa6-d2e775cc9f12";

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Digest::from_bytes(hash)
}

fn pinned_source(path: &str) -> Result<(Vec<u8>, Sha256Digest)> {
    let bytes = fs::read(path).context("read independently pinned original PUB")?;
    ensure!(bytes.len() == 291_840, "pinned source PUB size mismatch");
    let digest = source_hash(&bytes);
    ensure!(digest.to_string() == SOURCE_SHA, "pinned source PUB hash mismatch");
    Ok((bytes, digest))
}

fn pinned_font() -> Result<(Vec<u8>, FontResourceIdentityV1)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fonts/ofl/abel/Abel-Regular.ttf");
    let bytes = fs::read(path).context("load server-owned licensed complete font")?;
    ensure!(bytes.len() == 35_220, "pinned font resource size mismatch");
    ensure!(&bytes[..4] == &0x0001_0000_u32.to_be_bytes(), "not the pinned OpenType font");
    ensure!(format!("{:x}", Sha256::digest(&bytes)) == FONT_SHA, "pinned font SHA mismatch");
    let identity = FontResourceIdentityV1 {
        resource_id: FONT_ID.to_owned(),
        font_fingerprint: format!("sha256:{FONT_SHA}"),
        content_hash: FONT_SHA.to_owned(),
        face_index: 0,
    };
    Ok((bytes, identity))
}

fn resource<'a>(
    identity: &'a FontResourceIdentityV1,
    bytes: &'a [u8],
) -> ServerFontResourceV1<'a> {
    ServerFontResourceV1 {
        identity,
        full_font_bytes: bytes,
        face_count: 1,
        is_full_resource: true,
        authoring_admitted: true, // exact pinned OFL, only this explicit developer mode
    }
}

fn project_at(path: &str, source_sha: Sha256Digest) -> Result<EditorProject> {
    let project: EditorProject = serde_json::from_slice(
        &fs::read(path).context("read canonical EditorProject")?,
    ).context("parse canonical EditorProject")?;
    ensure!(project.source_hash == source_sha, "EditorProject source hash mismatch");
    ensure!(project.identity.is_some(), "font history requires identity-bearing project");
    Ok(project)
}

fn reopen(
    original: &[u8], source_sha: Sha256Digest, project: &EditorProject,
    bytes: &[u8], identity: &FontResourceIdentityV1,
) -> Result<EditorSession> {
    let mut session = open_mature_0x2c_editor(original, source_sha)
        .context("open real source-backed EditorSession")?;
    let grant = EditorProjectFontReopenGrantV1 {
        source_hash: source_sha,
        project_document_id: &project.identity.as_ref().context("missing document identity")?.document_id,
        resource: resource(identity, bytes),
    };
    session
        .apply_project_with_admitted_font_resources_v1(
            project, &BTreeMap::new(), &[grant],
        )
        .context("independently readmit exact recorded font history")?;
    Ok(session)
}

fn exact_scope(path: &str, project: &EditorProject) -> Result<FontAuthoringScopeV1> {
    let scope: serde_json::Value = serde_json::from_slice(
        &fs::read(path).context("read server-owned current font scope")?,
    ).context("parse independent current font scope")?;
    let object = scope.as_object().context("current font scope must be an object")?;
    let fields = [
        "document_id", "revision_id", "scene_snapshot_id",
        "layout_environment_id", "font_set_fingerprint",
    ];
    ensure!(
        object.len() == fields.len() && fields.iter().all(|k| object.contains_key(*k)),
        "current scope fields mismatch"
    );
    let get = |key: &str| -> Result<String> {
        Ok(scope.get(key).and_then(|value| value.as_str())
            .with_context(|| format!("current font scope {key} missing"))?
            .to_owned())
    };
    let out = FontAuthoringScopeV1 {
        document_id: get("document_id")?,
        revision_id: get("revision_id")?,
        scene_snapshot_id: get("scene_snapshot_id")?,
        layout_environment_id: get("layout_environment_id")?,
        font_set_fingerprint: get("font_set_fingerprint")?,
    };
    ensure!(
        project.identity.as_ref().is_some_and(|id| id.document_id == out.document_id),
        "server font scope belongs to a different canonical EditorProject"
    );
    Ok(out)
}

fn run_initialize(path: &str) -> Result<()> {
    let (source, hash) = pinned_source(path)?;
    let session = open_mature_0x2c_editor(&source, hash)
        .context("initialize identity-bearing real Publisher project")?;
    let project = session.project();
    ensure!(project.identity.is_some(), "missing canonical project identity");
    println!("{}", serde_json::to_string(&serde_json::json!({
        "protocol_version": "chaptera.pinned-font-project-init.v1",
        "source_hash": hash,
        "project": project,
    }))?);
    Ok(())
}

fn run_capabilities(source_path: &str, project_path: &str) -> Result<()> {
    let (source, hash) = pinned_source(source_path)?;
    let project = project_at(project_path, hash)?;
    let (font, identity) = pinned_font()?;
    let session = reopen(&source, hash, &project, &font, &identity)?;
    let ids: Vec<_> = session.graph().stories.keys().copied()
        .filter(|id| session.current_text_format_state_hash_v1(*id).is_ok())
        .collect();
    println!("{}", serde_json::to_string(&serde_json::json!({
        "protocol_version": "chaptera.pinned-font-editor-capabilities.v1",
        "source_hash": hash,
        "project_state_id": project.state_id_v1(),
        "font_editable_story_ids": ids,
    }))?);
    Ok(())
}

fn run_verify(source_path: &str, project_path: &str) -> Result<()> {
    let (source, hash) = pinned_source(source_path)?;
    let project = project_at(project_path, hash)?;
    let (font, identity) = pinned_font()?;
    let session = reopen(&source, hash, &project, &font, &identity)?;
    ensure!(session.project().state_id_v1() == project.state_id_v1(), "fresh font project state drift");
    println!("{}", serde_json::to_string(&serde_json::json!({
        "protocol_version": "chaptera.pinned-font-project-verified.v1",
        "source_hash": hash,
        "project_state_id": project.state_id_v1(),
        "operation_count": project.operations.len(),
        "full_font_re_admitted": true,
        "layout_reshaped": false,
        "fixed_pdf_allowed": false,
    }))?);
    Ok(())
}

fn run_apply(
    source_path: &str, project_path: &str, command_path: &str, scope_path: &str,
) -> Result<()> {
    let (source, hash) = pinned_source(source_path)?;
    let project = project_at(project_path, hash)?;
    let scope = exact_scope(scope_path, &project)?;
    let command: serde_json::Value = serde_json::from_slice(
        &fs::read(command_path).context("read bounded untrusted font intent")?,
    ).context("parse font intent JSON")?;
    let command_keys = command.as_object().context("font command must be object")?;
    ensure!(
        command_keys.len() == 5 && ["kind", "story_id", "start_scalar",
        "end_scalar", "candidate"].iter().all(|key| command_keys.contains_key(*key)),
        "unexpected or authority-bearing font command fields"
    );
    ensure!(command["kind"].as_str() == Some("set_admitted_font_resource"), "incorrect font operation");
    let story_id: StoryId = serde_json::from_value(command["story_id"].clone())
        .context("parse exact StoryId")?;
    let start = command["start_scalar"].as_u64().context("font start_scalar missing")?;
    let end = command["end_scalar"].as_u64().context("font end_scalar missing")?;
    let start = u32::try_from(start).context("font start_scalar overflow")?;
    let end = u32::try_from(end).context("font end_scalar overflow")?;
    ensure!(end > start, "font range must be nonempty");
    let candidate: FontReplacementCandidateV1 =
        serde_json::from_value(command["candidate"].clone())
            .context("parse untrusted exact font candidate")?;
    let (bytes, identity) = pinned_font()?;
    let mut session = reopen(&source, hash, &project, &bytes, &identity)?;
    let before_hash = session.current_text_format_state_hash_v1(story_id)
        .context("source Story format not fully admitted")?;
    let before_text = session.graph().stories.get(&story_id)
        .context("font Story missing")?.text.clone();
    let operation = session.set_admitted_font_resource_v1(
        story_id, start, end, &candidate, &scope,
        &resource(&identity, &bytes), &before_hash,
    ).context("commit canonical exact-byte FontResource in Rust")?;
    ensure!(
        session.graph().stories.get(&story_id).is_some_and(|v| v.text == before_text),
        "font operation unexpectedly changed source Story text"
    );
    println!("{}", serde_json::to_string(&serde_json::json!({
        "protocol_version": "chaptera.pinned-font-operation-result.v1",
        "source_hash": hash,
        "operation": operation,
        "project": session.project(),
        "source_text_unchanged": true,
        "authoritative_relayout": false,
        "fixed_pdf_allowed": false,
    }))?);
    Ok(())
}

pub fn run(mode: &str, arguments: Vec<String>) -> Result<()> {
    match (mode, arguments.as_slice()) {
        ("font-initialize", [source]) => run_initialize(source),
        ("font-capabilities", [source, project]) => run_capabilities(source, project),
        ("font-verify", [source, project]) => run_verify(source, project),
        ("font-apply", [source, project, command, scope]) => {
            run_apply(source, project, command, scope)
        }
        _ => anyhow::bail!("unsupported pinned font tool mode or arguments"),
    }
}
