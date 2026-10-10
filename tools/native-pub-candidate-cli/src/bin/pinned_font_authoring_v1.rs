//! Task-local trusted-worker bridge for the one pinned OFL font.
//!
//! This binary consumes a SERVER-OWNED Scene/EditorProject and an untrusted
//! exact-resource candidate. Its local JSON inputs are not network auth: an
//! HTTP caller must authenticate, authorize and obtain the Scene independently.
//! No native PUB/PDF output or authoritative text relayout is provided.
use anyhow::{Context, Result, bail, ensure};
use chaptera_text_format_overlay::{
    FontAuthoringScopeV1, FontReplacementCandidateV1, FontResourceIdentityV1,
    ServerFontResourceV1,
};
use pub_editor::{
    EditorProject, EditorProjectFontReopenGrantV1, Sha256Digest, StoryId,
    open_mature_0x2c_editor,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const SOURCE_SHA: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
const SOURCE_LEN: u64 = 291_840;
const FONT_SHA: &str = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4";
const FONT_ID: &str = "f27a8036-8492-480f-8fa6-d2e775cc9f12";

fn read_source(path: &str) -> Result<(Vec<u8>, Sha256Digest)> {
    ensure!(fs::metadata(path).context("source metadata unavailable")?.len() == SOURCE_LEN,
        "pinned Newsletter source size mismatch");
    let bytes = fs::read(path).context("read exact pinned PUB")?;
    ensure!(format!("{:x}", Sha256::digest(&bytes)) == SOURCE_SHA,
        "pinned Newsletter source hash mismatch");
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    Ok((bytes, Sha256Digest::from_bytes(digest)))
}

fn font_bytes() -> Result<Vec<u8>> {
    // Fixed, OFL-licensed complete physical resource from repository; neither
    // request/Scene nor host OS may choose a font file path.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fonts/ofl/abel/Abel-Regular.ttf");
    ensure!(fs::metadata(&file)?.len() == 35_220, "pinned font size mismatch");
    let bytes = fs::read(file).context("read pinned OFL font")?;
    ensure!(format!("{:x}", Sha256::digest(&bytes)) == FONT_SHA,
        "pinned font content SHA-256 mismatch");
    ensure!(bytes.starts_with(&[0, 1, 0, 0]), "pinned OpenType header mismatch");
    Ok(bytes)
}

fn font_identity() -> FontResourceIdentityV1 {
    FontResourceIdentityV1 {
        resource_id: FONT_ID.to_owned(),
        font_fingerprint: format!("sha256:{FONT_SHA}"),
        content_hash: FONT_SHA.to_owned(),
        face_index: 0,
    }
}

fn grant<'a>(id: &'a FontResourceIdentityV1, bytes: &'a [u8]) -> ServerFontResourceV1<'a> {
    ServerFontResourceV1 {
        identity: id,
        full_font_bytes: bytes,
        face_count: 1,
        is_full_resource: true,
        authoring_admitted: true, // one pinned OFL demo grant, never browser-provided
    }
}

fn open_project(
    source: &[u8],
    digest: Sha256Digest,
    project: &EditorProject,
    id: &FontResourceIdentityV1,
    bytes: &[u8],
) -> Result<pub_editor::EditorSession> {
    ensure!(project.source_hash == digest, "EditorProject/source identity mismatch");
    let project_doc_id = &project.identity.as_ref()
        .context("identity-bearing EditorProject required for font history")?.document_id;
    let mut session = open_mature_0x2c_editor(source, digest)
        .context("open real Publisher source")?;
    session.apply_project_with_admitted_font_resources_v1(
        project,
        &BTreeMap::new(),
        &[EditorProjectFontReopenGrantV1 {
            source_hash: digest,
            project_document_id: project_doc_id,
            resource: grant(id, bytes),
        }],
    ).context("fresh Project replay requires independent font re-admission")?;
    Ok(session)
}

fn read_project(path: &str) -> Result<EditorProject> {
    serde_json::from_slice(&fs::read(path).context("read EditorProject")?)
        .context("decode canonical EditorProject")
}

fn read_json(path: &str) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).context("read JSON input")?)
        .context("decode JSON input")
}

fn text(value: &Value, key: &str) -> Result<String> {
    value.get(key).and_then(Value::as_str)
        .map(str::to_owned).with_context(|| format!("missing string {key}"))
}

fn scope_from_scene(scene: &Value) -> Result<FontAuthoringScopeV1> {
    ensure!(text(scene, "source_hash")? == SOURCE_SHA, "trusted Scene source hash mismatch");
    let layout = scene.get("layout_environment")
        .context("trusted Scene missing layout environment")?;
    Ok(FontAuthoringScopeV1 {
        document_id: text(scene, "document_id")?,
        revision_id: text(scene, "revision_id")?,
        scene_snapshot_id: text(scene, "snapshot_id")?,
        layout_environment_id: text(layout, "environment_id")?,
        font_set_fingerprint: text(layout, "font_set_fingerprint")?,
    })
}

fn emit_project_initial(source_path: &str) -> Result<()> {
    let (source, digest) = read_source(source_path)?;
    let session = open_mature_0x2c_editor(&source, digest).context("open real Publisher Editor")?;
    let project = session.project();
    ensure!(project.identity.is_some(), "source-backed project identity absent");
    println!("{}", serde_json::to_string(&project)?);
    Ok(())
}

fn emit_probe(source_path: &str, project_path: &str) -> Result<()> {
    let (source, digest) = read_source(source_path)?;
    let project = read_project(project_path)?;
    let bytes = font_bytes()?;
    let id = font_identity();
    let session = open_project(&source, digest, &project, &id, &bytes)?;
    let mut stories = Vec::new();
    for (story_id, story) in &session.graph().stories {
        let scalar_len = story.text.chars().count();
        if scalar_len == 0 { continue; }
        if let Ok(state_hash) = session.current_text_format_state_hash_v1(*story_id) {
            stories.push(json!({
                "story_id": story_id, "story_scalar_len": scalar_len,
                "expected_state_hash": state_hash,
            }));
        }
    }
    ensure!(!stories.is_empty(), "real PUB has no provable font-editable Story");
    println!("{}", json!({
        "protocol_version": "chaptera.local-font-format-probe.v1",
        "source_hash": SOURCE_SHA,
        "project_state_id": session.project().state_id_v1(),
        "stories": stories,
    }));
    Ok(())
}

fn emit_apply(
    source_path: &str,
    project_path: &str,
    trusted_scene_path: &str,
    intent_path: &str,
) -> Result<()> {
    let (source, digest) = read_source(source_path)?;
    let project = read_project(project_path)?;
    let scene = read_json(trusted_scene_path)?;
    let context = scope_from_scene(&scene)?;
    let project_doc_id = &project.identity.as_ref()
        .context("font Project requires independently admitted document identity")?
        .document_id;
    ensure!(context.document_id == *project_doc_id,
        "trusted Scene document identity differs from actual EditorProject");
    let intent = read_json(intent_path)?;
    ensure!(text(&intent, "protocol_version")? == "chaptera.local-pinned-font-intent.v1",
        "unsupported local font-intent protocol");
    let candidate: FontReplacementCandidateV1 = serde_json::from_value(
        intent.get("candidate").cloned().context("candidate required")?
    ).context("invalid exact font replacement candidate")?;
    ensure!(candidate.resource_id == FONT_ID && candidate.content_hash == FONT_SHA,
        "unsupported candidate physical font identity");
    let story_id: StoryId = serde_json::from_value(
        intent.get("story_id").cloned().context("StoryId required")?
    ).context("invalid StoryId")?;
    let start_scalar = u32::try_from(
        intent.get("start_scalar").and_then(Value::as_u64)
            .context("start_scalar invalid")?
    ).context("start_scalar overflow")?;
    let end_scalar = u32::try_from(
        intent.get("end_scalar").and_then(Value::as_u64)
            .context("end_scalar invalid")?
    ).context("end_scalar overflow")?;
    ensure!(end_scalar > start_scalar, "font edit range must not be empty");
    let expected_state_hash = text(&intent, "expected_state_hash")?;
    let bytes = font_bytes()?;
    let id = font_identity();
    let mut session = open_project(&source, digest, &project, &id, &bytes)?;
    let original_text = session.graph().stories.get(&story_id)
        .context("target Story not in Publisher graph")?.text.clone();
    let operation = session.set_admitted_font_resource_v1(
        story_id, start_scalar, end_scalar, &candidate, &context,
        &grant(&id, &bytes), &expected_state_hash,
    ).context("Rust exact-byte font admission refused")?;
    let after = session.current_text_format_state_hash_v1(story_id)?;
    ensure!(after != expected_state_hash, "font state did not change");
    ensure!(session.graph().stories[&story_id].text == original_text,
        "font resource operation modified immutable Story text");
    let edited_project = session.project();
    // Same immutable source + independently supplied ORIGINAL physical bytes
    // must reproduce the canonical history; public bare replay must not.
    let fresh = open_project(&source, digest, &edited_project, &id, &bytes)?;
    ensure!(fresh.project().state_id_v1() == edited_project.state_id_v1(),
        "fresh physical-font re-admission diverged from saved Project");
    ensure!(fresh.current_text_format_state_hash_v1(story_id)? == after,
        "fresh physical-font Story override differs");
    println!("{}", json!({
        "protocol_version": "chaptera.local-pinned-font-apply.v1",
        "source_hash": SOURCE_SHA,
        "original_scene_revision_id": context.revision_id,
        "original_scene_snapshot_id": context.scene_snapshot_id,
        "story_id": story_id,
        "canonical_operation": operation,
        "project": edited_project,
        "project_state_id": fresh.project().state_id_v1(),
        "format_state_hash": after,
        "fresh_reopen_with_exact_bytes": true,
        "source_story_text_unchanged": true,
        "layout_authority": "partial_not_reshaped",
        "fixed_output_eligible": false,
    }));
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let mode = args.next().context("init/probe/apply mode required")?;
    let source = args.next().context("pinned source PUB path required")?;
    match mode.as_str() {
        "init" => {
            ensure!(args.next().is_none(), "init accepts only source path");
            emit_project_initial(&source)
        }
        "probe" => {
            let project = args.next().context("EditorProject path required")?;
            ensure!(args.next().is_none(), "unexpected probe arguments");
            emit_probe(&source, &project)
        }
        "apply" => {
            let project = args.next().context("EditorProject path required")?;
            let scene = args.next().context("trusted Scene path required")?;
            let intent = args.next().context("font candidate intent path required")?;
            ensure!(args.next().is_none(), "unexpected apply arguments");
            emit_apply(&source, &project, &scene, &intent)
        }
        _ => bail!("unsupported local font mode"),
    }
}
