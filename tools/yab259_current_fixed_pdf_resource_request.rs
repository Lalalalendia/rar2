use anyhow::{Context, Result, bail};
use pub_layout::{BoundedShapedFlowScene, BoundedShapedText, font_fingerprint_sha256};
use pub_model::LengthEmu;
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImageResource, FixedNodePaint, FixedPdfResources, FixedTextRun,
    PdfTargetProfile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{self, Read};

const INPUT_VERSION: &str = "chaptera.current-fixed-pdf-resource-input.v1";
const OUTPUT_VERSION: &str = "chaptera.fixed-pdf-packet-render-request.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentFont {
    fingerprint_sha256: String,
    face_index: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentResourceInput {
    protocol_version: String,
    binding: Value,
    shaped_flow: BoundedShapedFlowScene,
    node_paints: Vec<FixedNodePaint>,
    image_resources: Vec<FixedImageResource>,
    font: CurrentFont,
}

#[derive(Debug, Serialize)]
struct PacketRenderRequest {
    protocol_version: &'static str,
    binding: Value,
    scene: pub_layout::BoundedResolvedScene,
    resources: FixedPdfResources,
    target: PdfTargetProfile,
}

fn main() -> Result<()> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .context("read current fixed-PDF resource input")?;
    let input: CurrentResourceInput =
        serde_json::from_str(&raw).context("parse current fixed-PDF resource input")?;
    if input.protocol_version != INPUT_VERSION {
        bail!("unsupported current fixed-PDF resource input protocol");
    }
    if input.font.bytes.is_empty() {
        bail!("current fixed-PDF explicit font bytes are required");
    }

    let actual_fingerprint = font_fingerprint_sha256(&input.font.bytes);
    if actual_fingerprint != input.font.fingerprint_sha256 {
        bail!("current fixed-PDF explicit font fingerprint mismatch");
    }
    if input.shaped_flow.environment.shaping.face_index != input.font.face_index {
        bail!("current fixed-PDF font face differs from shaped-flow environment");
    }
    if input
        .shaped_flow
        .environment
        .shaping
        .layout
        .font_set_fingerprint
        != input.font.fingerprint_sha256
    {
        bail!("current fixed-PDF font identity differs from shaped-flow environment");
    }

    let mut text_runs = Vec::new();
    let mut used_glyph_ids = BTreeSet::new();
    for line in &input.shaped_flow.lines {
        if line.text.is_empty() {
            continue;
        }
        if line.units_per_em == 0 {
            bail!("current shaped line units_per_em must be positive");
        }
        if line.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            bail!("current shaped line contains a missing glyph");
        }

        let row_offset = i64::from(line.frame_line_index)
            .checked_mul(input.shaped_flow.environment.line_height.get())
            .context("current fixed-PDF row offset overflow")?;
        let baseline_y = row_offset
            .checked_add(input.shaped_flow.environment.shaping.font_size_emu.get())
            .context("current fixed-PDF baseline overflow")?;

        used_glyph_ids.extend(line.glyphs.iter().map(|glyph| glyph.glyph_id));
        text_runs.push(FixedTextRun {
            node_id: line.frame_origin,
            scalar_base: line.scalar_start,
            logical_text: line.text.clone(),
            shaped: BoundedShapedText {
                environment: input.shaped_flow.environment.shaping.clone(),
                units_per_em: line.units_per_em,
                glyphs: line.glyphs.clone(),
                total_x_advance: line.measured_width,
            },
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(baseline_y),
            fill_rgb: [0, 0, 0],
        });
    }

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let embedding = read_opentype_embedding_flags(&input.font.bytes, input.font.face_index)
            .context("read current fixed-PDF font embedding flags")?;
        let identity = FontIdentity {
            fingerprint_sha256: input.font.fingerprint_sha256.clone(),
            face_index: input.font.face_index,
        };
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: identity.clone(),
                    bytes: &input.font.bytes,
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
            bail!("current fixed-PDF font cannot be embedded: {codes}");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity,
                bytes: input.font.bytes.clone(),
            }],
        )
    };

    let request = PacketRenderRequest {
        protocol_version: OUTPUT_VERSION,
        binding: input.binding,
        scene: input.shaped_flow.geometry_scene(),
        resources: FixedPdfResources {
            node_paints: input.node_paints,
            images: input.image_resources,
            font_plan,
            fonts,
            text_runs,
        },
        target: PdfTargetProfile::basic_geometry_v0_1(),
    };

    println!("{}", serde_json::to_string(&request)?);
    Ok(())
}
