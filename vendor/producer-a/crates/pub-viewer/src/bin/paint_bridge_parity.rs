use anyhow::{Context, Result, bail};
use pub_layout::BoundedLayoutEnvironment;
use pub_model::Sha256Digest;
use pub_reader::{PubNodePayload, build_mature_0x2c_source_graph};
use pub_viewer::open_mature_0x2c_geometry;
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor};

const MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1: i64 = 0x0132_F540;

type ExpectedPaint = (Option<[u8; 3]>, Option<([u8; 3], i64)>, bool);

fn expected_paint(payload: &PubNodePayload) -> ExpectedPaint {
    if let Some(effective) = payload.effective_paint.as_ref() {
        let fill = match (
            effective.fill.solid.as_ref(),
            effective.fill.color_rgb.as_ref(),
            effective.fill.visible.as_ref(),
        ) {
            (Some(solid), Some(color), Some(visible)) if solid.value && visible.value => {
                Some(color.value)
            }
            _ => None,
        };
        let line = match (
            effective.line.color_rgb.as_ref(),
            effective.line.width_emu.as_ref(),
            effective.line.visible.as_ref(),
        ) {
            (Some(color), Some(width), Some(visible))
                if visible.value
                    && width.value > 0
                    && width.value <= MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1 =>
            {
                Some((color.value, width.value))
            }
            _ => None,
        };
        return (fill, line, true);
    }

    let paint = &payload.explicit_paint;
    let fill = (paint.fill.solid && paint.fill.visible == Some(true))
        .then_some(paint.fill.color_rgb)
        .flatten();
    let line = match (
        paint.line.visible,
        paint.line.color_rgb,
        paint.line.width_emu,
    ) {
        (Some(true), Some(rgb), Some(width_emu))
            if width_emu > 0 && width_emu <= MAX_BOUNDED_SOURCE_LINE_WIDTH_EMU_V1 =>
        {
            Some((rgb, width_emu))
        }
        _ => None,
    };
    (fill, line, false)
}

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: paint_bridge_parity <file.pub>")?;
    let bytes = fs::read(&path).with_context(|| format!("read {path}"))?;

    let digest = Sha256::digest(&bytes);
    let mut hash_bytes = [0_u8; 32];
    hash_bytes.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(hash_bytes);

    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .context("build source graph")?;
    let viewer = open_mature_0x2c_geometry(
        &bytes,
        BoundedLayoutEnvironment {
            engine_revision: "paint-bridge-parity-v2".to_owned(),
            font_set_fingerprint: "paint-bridge-parity-v2".to_owned(),
            resource_fingerprint: "paint-bridge-parity-v2".to_owned(),
        },
    )
    .context("open Viewer geometry")?;

    let mut effective_payload_nodes = 0_usize;
    let mut effective_paintable = 0_usize;
    let mut explicit_fallback_paintable = 0_usize;
    let mut expected_paintable = 0_usize;
    let mut compared = 0_usize;
    let mut mismatches = Vec::new();

    for node in source.graph.nodes.values() {
        let (expected_fill, expected_line, used_effective) = expected_paint(&node.payload);
        if used_effective {
            effective_payload_nodes += 1;
        }
        if expected_fill.is_none() && expected_line.is_none() {
            continue;
        }
        expected_paintable += 1;
        if used_effective {
            effective_paintable += 1;
        } else {
            explicit_fallback_paintable += 1;
        }

        let projected = viewer
            .paints
            .iter()
            .find(|candidate| candidate.node_id == node.header.id);
        let Some(projected) = projected else {
            mismatches.push(format!(
                "{}: expected bridge paint is absent in Viewer",
                node.header.id.as_canonical()
            ));
            continue;
        };
        compared += 1;

        let projected_line = projected
            .solid_line
            .as_ref()
            .map(|line| (line.rgb, line.width_emu));
        if projected.solid_fill_rgb != expected_fill || projected_line != expected_line {
            mismatches.push(format!(
                "{}: expected fill={expected_fill:?} line={expected_line:?}; Viewer fill={:?} line={projected_line:?}",
                node.header.id.as_canonical(),
                projected.solid_fill_rgb
            ));
        }
    }

    let unexpected = viewer
        .paints
        .iter()
        .filter(|candidate| {
            !source.graph.nodes.values().any(|node| {
                if node.header.id != candidate.node_id {
                    return false;
                }
                let (fill, line, _) = expected_paint(&node.payload);
                fill.is_some() || line.is_some()
            })
        })
        .count();

    println!(
        "PAINT_BRIDGE_PARITY\teffective_payload_nodes={effective_payload_nodes}\teffective_paintable={effective_paintable}\texplicit_fallback_paintable={explicit_fallback_paintable}\texpected_paintable={expected_paintable}\tviewer_paints={}\tcompared={compared}\tmismatches={}\tunexpected={unexpected}",
        viewer.paints.len(),
        mismatches.len()
    );
    for mismatch in &mismatches {
        eprintln!("PAINT_BRIDGE_MISMATCH\t{mismatch}");
    }

    if !mismatches.is_empty() || unexpected != 0 || viewer.paints.len() != expected_paintable {
        bail!("canonical paint bridge parity failed");
    }
    Ok(())
}
