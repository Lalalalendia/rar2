use anyhow::{Context, Result, bail};
use pub_layout::{BoundedShapedFlowScene, BoundedShapedText, font_fingerprint_sha256};
use pub_model::{LengthEmu, NodeId, StoryId};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImageResource, FixedNodePaint, FixedPdfResources, FixedTextRun,
    PdfRenderDisposition, PdfTargetProfile, render_bounded_pdf,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env,
    fs,
    io::{self, Read},
};

const REQUEST_VERSION: &str = "chaptera.editor-fixed-pdf-render-request.v1";
const PACKET_VERSION: &str = "chaptera.desktop-fixed-output-packet.v1";
const RESULT_VERSION: &str = "chaptera.editor-fixed-pdf-render-result.v1";

#[derive(Debug, Deserialize)]
struct RenderRequest {
    protocol_version: String,
    source_hash: String,
    project_hash: String,
    packet_id: String,
    scene_snapshot_id: String,
    flow_id: String,
    packet: DesktopFixedOutputPacketV1,
}

#[derive(Debug, Deserialize)]
struct DesktopFixedOutputPacketV1 {
    protocol_version: String,
    source_hash: String,
    project_state_id: String,
    story_states: Vec<DesktopFixedOutputStoryStateV1>,
    story_mutation_ids: Vec<StoryId>,
    move_node_ids: Vec<NodeId>,
    resize_node_ids: Vec<NodeId>,
    replacement_node_ids: Vec<NodeId>,
    shaped_flow: BoundedShapedFlowScene,
    node_paints: Vec<FixedNodePaint>,
    image_resources: Vec<FixedImageResource>,
    font: DesktopFixedOutputFontV1,
    invariants: DesktopFixedOutputInvariantsV1,
}

#[derive(Debug, Deserialize)]
struct DesktopFixedOutputStoryStateV1 {
    story_id: StoryId,
    story_state_id: String,
    scalar_count: u32,
}

#[derive(Debug, Deserialize)]
struct DesktopFixedOutputFontV1 {
    resource_id: String,
    fingerprint_sha256: String,
    face_index: u32,
    font_size_emu: LengthEmu,
    line_height_emu: LengthEmu,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct DesktopFixedOutputInvariantsV1 {
    authoritative_rust_project_replay: bool,
    source_reparse_after_project_apply_count: u32,
    source_refs_in_renderer_packet: bool,
    output_adapter_reshaping_calls: u32,
}

#[derive(Debug, Serialize)]
struct RenderSummary {
    page_count: usize,
    node_painted: usize,
    node_partial: usize,
    node_unsupported: usize,
    diagnostic_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RenderResult {
    protocol_version: &'static str,
    source_hash: String,
    project_hash: String,
    packet_id: String,
    scene_snapshot_id: String,
    flow_id: String,
    renderer_revision: &'static str,
    target_profile: &'static str,
    summary: RenderSummary,
}

fn validate_packet(request: &RenderRequest) -> Result<()> {
    if request.protocol_version != REQUEST_VERSION {
        bail!("renderer request protocol_version mismatch");
    }
    let packet = &request.packet;
    if packet.protocol_version != PACKET_VERSION {
        bail!("desktop fixed-output packet protocol_version mismatch");
    }
    if packet.source_hash != request.source_hash {
        bail!("desktop fixed-output packet source_hash mismatch");
    }
    if packet.project_state_id.is_empty() {
        bail!("desktop fixed-output packet project_state_id is required");
    }
    if !packet.invariants.authoritative_rust_project_replay
        || packet.invariants.source_reparse_after_project_apply_count != 0
        || packet.invariants.source_refs_in_renderer_packet
        || packet.invariants.output_adapter_reshaping_calls != 0
    {
        bail!("desktop fixed-output packet invariants reject renderer admission");
    }
    if packet.font.resource_id.is_empty() || packet.font.bytes.is_empty() {
        bail!("desktop fixed-output packet explicit font is missing");
    }

    let actual_fingerprint = font_fingerprint_sha256(&packet.font.bytes);
    if actual_fingerprint != packet.font.fingerprint_sha256 {
        bail!("desktop fixed-output packet font fingerprint mismatch");
    }
    if packet.shaped_flow.environment.shaping.font_size_emu != packet.font.font_size_emu {
        bail!("desktop fixed-output packet font size differs from shaped-flow environment");
    }
    if packet.shaped_flow.environment.shaping.face_index != packet.font.face_index {
        bail!("desktop fixed-output packet face index differs from shaped-flow environment");
    }
    if packet.shaped_flow.environment.line_height != packet.font.line_height_emu {
        bail!("desktop fixed-output packet line height differs from shaped-flow environment");
    }
    if packet.shaped_flow.environment.shaping.layout.font_set_fingerprint
        != packet.font.fingerprint_sha256
    {
        bail!("desktop fixed-output packet font identity differs from shaped-flow environment");
    }

    for story_id in &packet.story_mutation_ids {
        let state = packet
            .story_states
            .iter()
            .find(|state| state.story_id == *story_id)
            .ok_or_else(|| anyhow::anyhow!("mutated Story has no current state identity"))?;
        if state.story_state_id.is_empty() || state.scalar_count == 0 {
            bail!("mutated Story current state is invalid");
        }
        if !packet
            .shaped_flow
            .lines
            .iter()
            .any(|line| line.story_origin == *story_id && !line.text.is_empty())
        {
            bail!("mutated Story has no materialized current shaped line");
        }
    }
    Ok(())
}

fn materialize_text_runs(
    packet: &DesktopFixedOutputPacketV1,
) -> Result<(Vec<FixedTextRun>, BTreeSet<u16>)> {
    let mut runs = Vec::new();
    let mut used_glyph_ids = BTreeSet::new();

    for line in &packet.shaped_flow.lines {
        if line.text.is_empty() {
            continue;
        }
        if line.units_per_em == 0 {
            bail!("shaped line units_per_em must be positive");
        }
        if line.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            bail!("current edited Story contains a glyph missing from the explicit font");
        }

        let row_offset = i64::from(line.frame_line_index)
            .checked_mul(packet.font.line_height_emu.get())
            .context("current fixed-output row offset overflow")?;
        let baseline_y = row_offset
            .checked_add(packet.font.font_size_emu.get())
            .context("current fixed-output baseline overflow")?;

        used_glyph_ids.extend(line.glyphs.iter().map(|glyph| glyph.glyph_id));
        runs.push(FixedTextRun {
            node_id: line.frame_origin,
            scalar_base: line.scalar_start,
            logical_text: line.text.clone(),
            shaped: BoundedShapedText {
                environment: packet.shaped_flow.environment.shaping.clone(),
                units_per_em: line.units_per_em,
                glyphs: line.glyphs.clone(),
                total_x_advance: line.measured_width,
            },
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(baseline_y),
            fill_rgb: [0, 0, 0],
        });
    }

    Ok((runs, used_glyph_ids))
}

fn render(request: RenderRequest) -> Result<RenderResult> {
    validate_packet(&request)?;
    let packet = &request.packet;
    let (text_runs, used_glyph_ids) = materialize_text_runs(packet)?;

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let embedding = read_opentype_embedding_flags(&packet.font.bytes, packet.font.face_index)
            .context("read explicit current-editor font embedding flags")?;
        let identity = FontIdentity {
            fingerprint_sha256: packet.font.fingerprint_sha256.clone(),
            face_index: packet.font.face_index,
        };
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: identity.clone(),
                    bytes: &packet.font.bytes,
                    embedding,
                }),
                fallback_resource: None,
                used_glyph_ids,
            }],
        );
        if !plan.can_serialize() {
            let codes = plan
                .blockers
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("current-editor font cannot be embedded under fixed PDF policy: {codes}");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity,
                bytes: packet.font.bytes.clone(),
            }],
        )
    };

    let resources = FixedPdfResources {
        node_paints: packet.node_paints.clone(),
        images: packet.image_resources.clone(),
        font_plan,
        fonts,
        text_runs,
    };

    let rendered = render_bounded_pdf(
        &packet.shaped_flow.geometry_scene(),
        &resources,
        &PdfTargetProfile::basic_geometry_v0_1(),
    )
    .context("render current Editor packet with recovered pub-pdf")?;

    let require_painted = |node_id: &NodeId, label: &str| -> Result<()> {
        let report = rendered
            .report
            .nodes
            .iter()
            .find(|node| node.origin == *node_id)
            .ok_or_else(|| anyhow::anyhow!("{label} target is absent from PDF render report"))?;
        if report.disposition != PdfRenderDisposition::Painted {
            bail!(
                "{label} target was not painted by fixed PDF backend: code={}",
                report.code
            );
        }
        Ok(())
    };

    for node_id in &packet.move_node_ids {
        require_painted(node_id, "MoveNode")?;
    }
    for node_id in &packet.resize_node_ids {
        require_painted(node_id, "ResizeNode")?;
    }
    for node_id in &packet.replacement_node_ids {
        require_painted(node_id, "ReplaceImage")?;
        let resource_count = packet
            .image_resources
            .iter()
            .filter(|resource| resource.node_ids.contains(node_id))
            .count();
        if resource_count != 1 {
            bail!(
                "ReplaceImage target must bind exactly one effective image resource, found {resource_count}"
            );
        }
    }

    let output_path = env::var_os("CHAPTERA_PDF_OUTPUT")
        .ok_or_else(|| anyhow::anyhow!("CHAPTERA_PDF_OUTPUT is required"))?;
    fs::write(&output_path, &rendered.bytes).context("write current Editor PDF artifact")?;

    let mut diagnostic_codes = rendered
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    diagnostic_codes.sort();

    let summary = RenderSummary {
        page_count: rendered.report.pages.len(),
        node_painted: rendered
            .report
            .nodes
            .iter()
            .filter(|node| node.disposition == PdfRenderDisposition::Painted)
            .count(),
        node_partial: rendered
            .report
            .nodes
            .iter()
            .filter(|node| node.disposition == PdfRenderDisposition::Partial)
            .count(),
        node_unsupported: rendered
            .report
            .nodes
            .iter()
            .filter(|node| node.disposition == PdfRenderDisposition::Unsupported)
            .count(),
        diagnostic_codes,
    };

    Ok(RenderResult {
        protocol_version: RESULT_VERSION,
        source_hash: request.source_hash,
        project_hash: request.project_hash,
        packet_id: request.packet_id,
        scene_snapshot_id: request.scene_snapshot_id,
        flow_id: request.flow_id,
        renderer_revision: "pub-pdf-v0.1-yab259-editor-packet",
        target_profile: "basic-fixed-current-editor",
        summary,
    })
}

fn main() -> Result<()> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .context("read current Editor render request")?;
    let request: RenderRequest =
        serde_json::from_str(&raw).context("decode current Editor render request")?;
    let result = render(request)?;
    println!(
        "{}",
        serde_json::to_string(&result).context("encode current Editor render result")?
    );
    Ok(())
}
