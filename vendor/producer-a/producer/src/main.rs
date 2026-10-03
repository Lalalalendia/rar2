mod overset;

use anyhow::{Context, Result};
use pub_editor::{EditorEditableTarget, EditorProject, LengthEmu, NodeId, StoryId};
use pub_model::{Sha256Digest, to_cdm_debug_json_v0_1};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor, path::Path};

const SAMPLE_HASH: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";

fn pinned_hash() -> Sha256Digest {
    SAMPLE_HASH
        .parse()
        .expect("pinned SampleNewsletter SHA-256")
}

fn source_sha256(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&digest);
    Sha256Digest::from_bytes(value)
}

fn emit_viewer(path: &str) -> Result<()> {
    let bytes = fs::read(path).context("read pinned PUB fixture")?;
    let visual = pub_viewer::open_mature_0x2c_geometry(
        &bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .context("open mature-0x2c Viewer geometry")?;
    print!("{}", serde_json::to_string(&visual)?);
    Ok(())
}

fn emit_resolved_graph(path: &str) -> Result<()> {
    let bytes = fs::read(path).context("read pinned PUB fixture")?;
    let source =
        pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), pinned_hash())
            .context("build mature-0x2c SourceGraph")?;
    let resolved =
        pub_reader::resolve_pub_source_graph(&source.graph).context("resolve PUB SourceGraph")?;
    let canonical =
        to_cdm_debug_json_v0_1(&resolved.graph).context("serialize canonical resolved graph")?;
    std::io::Write::write_all(&mut std::io::stdout(), &canonical)?;
    Ok(())
}

fn emit_source_page_paint_orders(path: &str) -> Result<()> {
    let bytes = fs::read(path).context("read pinned PUB fixture")?;
    let source_hash = pinned_hash();
    let source = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash,
    )
    .context("build mature-0x2c SourceGraph")?;
    print!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema_version": "chaptera.pub-source-page-paint-orders-sidecar.v1",
            "source_hash": source_hash,
            "orders": source.source_page_paint_orders,
        }))?
    );
    Ok(())
}

fn editable_target(value: &str) -> Result<EditorEditableTarget> {
    match value {
        "idml" => Ok(EditorEditableTarget::Idml),
        "odg" => Ok(EditorEditableTarget::Odg),
        other => anyhow::bail!("unsupported editable target {other:?}"),
    }
}

fn emit_editable_export(
    fixture: &str,
    project_path: &str,
    target_name: &str,
    output_path: &str,
    report_path: &str,
) -> Result<()> {
    let bytes = fs::read(fixture).context("read pinned PUB fixture")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    let target = editable_target(target_name)?;
    let mut session =
        pub_editor::open_mature_0x2c_editor(&bytes, pinned_hash()).context("open editor")?;
    session
        .apply_project(&project)
        .context("replay canonical EditorProject")?;
    let preview = session
        .preview_editable_export(target, "SampleNewsletter.pub")
        .context("preview editable export")?;
    let export = session
        .export_editable(target, "SampleNewsletter.pub")
        .context("serialize editable export")?;
    if preview.report != export.report {
        anyhow::bail!("preview/export report mismatch");
    }
    fs::write(Path::new(output_path), &export.bytes).context("write editable export")?;
    fs::write(
        Path::new(report_path),
        serde_json::to_vec_pretty(&export.report).context("serialize export report")?,
    )
    .context("write export report")?;
    print!(
        "{}",
        serde_json::json!({
            "target": target_name,
            "project": project,
            "report": export.report,
            "byte_len": export.bytes.len(),
        })
    );
    Ok(())
}


fn emit_editor_capabilities(
    fixture: &str,
    project_path: &str,
) -> Result<()> {
    let bytes = fs::read(fixture).context("read source PUB fixture")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    if source_sha256(&bytes) != project.source_hash {
        anyhow::bail!("source PUB SHA-256 does not match EditorProject");
    }

    let mut session =
        pub_editor::open_mature_0x2c_editor(&bytes, project.source_hash).context("open editor")?;
    session
        .apply_project(&project)
        .context("replay canonical EditorProject")?;

    let mut editable_story_ids = session
        .graph()
        .stories
        .keys()
        .copied()
        .filter(|story_id| session.can_replace_story_text(*story_id).is_ok())
        .collect::<Vec<_>>();
    editable_story_ids.sort();

    print!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "protocol_version": "chaptera.editor-capabilities.v1",
            "source_hash": project.source_hash,
            "editable_story_ids": editable_story_ids,
        }))?
    );
    Ok(())
}

fn emit_editor_move_node(
    fixture: &str,
    project_path: &str,
    command_path: &str,
) -> Result<()> {
    let bytes = fs::read(fixture).context("read source PUB fixture")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    if source_sha256(&bytes) != project.source_hash {
        anyhow::bail!("source PUB SHA-256 does not match EditorProject");
    }
    let command: serde_json::Value =
        serde_json::from_slice(&fs::read(command_path).context("read MoveNode command")?)
            .context("parse MoveNode command")?;
    if command.get("kind").and_then(|value| value.as_str()) != Some("move_node_to") {
        anyhow::bail!("move_node_to command required");
    }

    let node_id: NodeId = serde_json::from_value(
        command
            .get("node_id")
            .cloned()
            .context("node_id missing")?,
    )
    .context("parse node_id")?;
    let x = command
        .get("x_emu")
        .and_then(|value| value.as_i64())
        .context("x_emu missing or invalid")?;
    let y = command
        .get("y_emu")
        .and_then(|value| value.as_i64())
        .context("y_emu missing or invalid")?;

    let mut session =
        pub_editor::open_mature_0x2c_editor(&bytes, project.source_hash).context("open editor")?;
    session
        .apply_project(&project)
        .context("replay canonical EditorProject")?;
    let operation = session
        .move_node_to(node_id, LengthEmu::new(x), LengthEmu::new(y))
        .context("apply bounded MoveNode")?;

    print!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "protocol_version": "chaptera.editor-move-node-result.v1",
            "source_hash": project.source_hash,
            "operation": operation,
            "project": session.project(),
        }))?
    );
    Ok(())
}

fn emit_editor_story_range(
    fixture: &str,
    project_path: &str,
    command_path: &str,
) -> Result<()> {
    let bytes = fs::read(fixture).context("read source PUB fixture")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    if source_sha256(&bytes) != project.source_hash {
        anyhow::bail!("source PUB SHA-256 does not match EditorProject");
    }
    let command: serde_json::Value =
        serde_json::from_slice(&fs::read(command_path).context("read Story range command")?)
            .context("parse Story range command")?;
    if command.get("kind").and_then(|value| value.as_str()) != Some("replace_story_range") {
        anyhow::bail!("replace_story_range command required");
    }

    let story_id: StoryId = serde_json::from_value(
        command
            .get("story_id")
            .cloned()
            .context("story_id missing")?,
    )
    .context("parse story_id")?;
    let start_scalar = u32::try_from(
        command
            .get("start_scalar")
            .and_then(|value| value.as_u64())
            .context("start_scalar missing or invalid")?,
    )
    .context("start_scalar exceeds u32")?;
    let end_scalar = u32::try_from(
        command
            .get("end_scalar")
            .and_then(|value| value.as_u64())
            .context("end_scalar missing or invalid")?,
    )
    .context("end_scalar exceeds u32")?;
    let expected_before = command
        .get("expected_before")
        .and_then(|value| value.as_str())
        .context("expected_before missing or invalid")?
        .to_owned();
    let replacement_text = command
        .get("replacement_text")
        .and_then(|value| value.as_str())
        .context("replacement_text missing or invalid")?
        .to_owned();

    let mut session =
        pub_editor::open_mature_0x2c_editor(&bytes, project.source_hash).context("open editor")?;
    session
        .apply_project(&project)
        .context("replay canonical EditorProject")?;
    let before_text = session
        .graph()
        .stories
        .get(&story_id)
        .context("Story missing from Editor graph")?
        .text
        .clone();
    let operation = session
        .replace_story_range(
            story_id,
            start_scalar,
            end_scalar,
            expected_before,
            replacement_text,
        )
        .context("apply bounded Story range")?;
    let after_text = session
        .graph()
        .stories
        .get(&story_id)
        .context("edited Story missing from Editor graph")?
        .text
        .clone();

    print!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "protocol_version": "chaptera.editor-story-range-result.v1",
            "source_hash": project.source_hash,
            "story_id": story_id,
            "before_text": before_text,
            "after_text": after_text,
            "operation": operation,
            "project": session.project(),
        }))?
    );
    Ok(())
}

fn emit_native_pub_save(
    fixture: &str,
    project_path: &str,
    output_path: &str,
    report_path: &str,
) -> Result<()> {
    let bytes = fs::read(fixture).context("read source PUB fixture")?;
    let project: EditorProject =
        serde_json::from_slice(&fs::read(project_path).context("read canonical EditorProject")?)
            .context("parse canonical EditorProject")?;
    let mut session = pub_editor::open_mature_0x2c_editor(&bytes, project.source_hash)
        .context("open bounded native PUB editor")?;
    session
        .apply_project(&project)
        .context("replay canonical EditorProject")?;

    let candidate = match session.materialize_mature_0x2c_native_pub_candidate(&bytes) {
        Ok(candidate) => candidate,
        Err(error) => {
            let report = serde_json::json!({
                "protocol_version": "chaptera.native-pub-save.v1",
                "source_hash": project.source_hash,
                "can_serialize": false,
                "blocker_code": error.code(),
                "chaptera_reopen_verified": false,
                "native_publisher_acceptance": "not_evaluated",
            });
            fs::write(
                Path::new(report_path),
                serde_json::to_vec_pretty(&report).context("serialize native PUB save report")?,
            )
            .context("write native PUB save report")?;
            print!("{}", serde_json::to_string(&report)?);
            return Ok(());
        }
    };

    let report = serde_json::json!({
        "protocol_version": "chaptera.native-pub-save.v1",
        "source_hash": candidate.source_hash,
        "output_hash": candidate.output_hash,
        "source_story_id": candidate.source_story_id,
        "output_story_id": candidate.output_story_id,
        "byte_len": candidate.bytes.len(),
        "can_serialize": true,
        "blocker_code": serde_json::Value::Null,
        "chaptera_reopen_verified": true,
        "native_publisher_acceptance": "not_evaluated",
    });
    fs::write(Path::new(output_path), &candidate.bytes).context("write native PUB candidate")?;
    fs::write(
        Path::new(report_path),
        serde_json::to_vec_pretty(&report).context("serialize native PUB save report")?,
    )
    .context("write native PUB save report")?;
    print!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let first = args
        .next()
        .context("fixture path or mode argument missing")?;
    if first == "authoring-overset" {
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return overset::run();
    }
    if first == "resolved-graph" {
        let path = args.next().context("fixture path argument missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_resolved_graph(&path);
    }
    if first == "source-page-paint-orders" {
        let path = args.next().context("fixture path argument missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_source_page_paint_orders(&path);
    }
    if first == "editable-export" {
        let fixture = args.next().context("fixture path missing")?;
        let project = args.next().context("project path missing")?;
        let target = args.next().context("target missing")?;
        let output = args.next().context("output path missing")?;
        let report = args.next().context("report path missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_editable_export(&fixture, &project, &target, &output, &report);
    }
    if first == "editor-capabilities" {
        let fixture = args.next().context("fixture path missing")?;
        let project = args.next().context("project path missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_editor_capabilities(&fixture, &project);
    }
    if first == "editor-move-node" {
        let fixture = args.next().context("fixture path missing")?;
        let project = args.next().context("project path missing")?;
        let command = args.next().context("command path missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_editor_move_node(&fixture, &project, &command);
    }
    if first == "editor-story-range" {
        let fixture = args.next().context("fixture path missing")?;
        let project = args.next().context("project path missing")?;
        let command = args.next().context("command path missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_editor_story_range(&fixture, &project, &command);
    }
    if first == "native-pub-save" {
        let fixture = args.next().context("fixture path missing")?;
        let project = args.next().context("project path missing")?;
        let output = args.next().context("output path missing")?;
        let report = args.next().context("report path missing")?;
        if args.next().is_some() {
            anyhow::bail!("unexpected extra arguments");
        }
        return emit_native_pub_save(&fixture, &project, &output, &report);
    }
    if args.next().is_some() {
        anyhow::bail!("unexpected extra arguments");
    }
    emit_viewer(&first)
}
