use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    EditorEditableTarget, EditorProject, EditorSession, LengthEmu, NodeId, RectEmu, Sha256Digest,
    Srgb8V1, open_mature_0x2c_editor,
};
use pub_export::{
    AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE,
    AUTHORED_SHAPE_STROKE_FEATURE, AUTHORED_SHAPE_Z_ORDER_FEATURE, CapabilityLevel,
};
use pub_model::CanonicalId;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn authored_node_id() -> NodeId {
    let mut bytes = [
        0x01, 0x9a, 0x2d, 0x80, 0x11, 0x12, 0x70, 0x01, 0x80, 0x01, 0xaa, 0xbb, 0xcc, 0xdd,
        0xee, 0x01,
    ];
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    NodeId::from_canonical(CanonicalId::from_bytes(bytes))
}

fn verify_report(
    report: &pub_export::ExportReport,
    node_id: NodeId,
) -> Result<(), Box<dyn Error>> {
    let origin = node_id.as_canonical().to_string();
    for feature in [
        AUTHORED_SHAPE_GEOMETRY_FEATURE,
        AUTHORED_SHAPE_FILL_FEATURE,
        AUTHORED_SHAPE_STROKE_FEATURE,
    ] {
        let item = report
            .items
            .iter()
            .find(|item| item.origin.as_deref() == Some(origin.as_str()) && item.feature == feature)
            .ok_or_else(|| format!("missing {feature} report item"))?;
        if item.disposition != CapabilityLevel::Preserved {
            return Err(format!("{feature} is not preserved: {:?}", item.disposition).into());
        }
    }
    let z_order = report
        .items
        .iter()
        .find(|item| {
            item.origin.as_deref() == Some(origin.as_str())
                && item.feature == AUTHORED_SHAPE_Z_ORDER_FEATURE
        })
        .ok_or("missing authored_shape.z_order report item")?;
    if z_order.disposition != CapabilityLevel::Unsupported {
        return Err(format!("z-order unexpectedly {:?}", z_order.disposition).into());
    }
    Ok(())
}

fn export_target(
    session: &EditorSession,
    target: EditorEditableTarget,
    label: &str,
    out: &std::path::Path,
    node_id: NodeId,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let export = session.export_editable(target, label.to_owned())?;
    verify_report(&export.report, node_id)?;
    let name = match target {
        EditorEditableTarget::Idml => "idml",
        EditorEditableTarget::Odg => "odg",
    };
    fs::write(out.join(format!("output.{name}")), &export.bytes)?;
    fs::write(
        out.join(format!("{name}.report.json")),
        serde_json::to_vec_pretty(&export.report)?,
    )?;
    Ok(json!({
        "bytes": export.bytes.len(),
        "blocking": export.report.counts.blocking,
        "semantic_losses": export.report.counts.semantic,
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let input = PathBuf::from(args.next().ok_or("usage: authored_shape_export_probe INPUT OUT_DIR")?);
    let out = PathBuf::from(args.next().ok_or("usage: authored_shape_export_probe INPUT OUT_DIR")?);
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    fs::create_dir_all(&out)?;

    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let mut session = open_mature_0x2c_editor(&bytes, hash)?;
    let page_id = *session
        .graph()
        .document
        .pages
        .first()
        .ok_or("source has no page")?;
    let node_id = authored_node_id();
    let bounds = RectEmu::new(
        LengthEmu::new(914_400),
        LengthEmu::new(914_400),
        LengthEmu::new(1_828_800),
        LengthEmu::new(914_400),
    );
    let paint = AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
            },
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    };

    session.create_shape(node_id, page_id, bounds, paint.clone())?;
    let project = session.project();
    let project_json = serde_json::to_vec_pretty(&project)?;
    fs::write(out.join("editor-project.json"), &project_json)?;

    let persisted: EditorProject = serde_json::from_slice(&project_json)?;
    let mut reopened = open_mature_0x2c_editor(&bytes, hash)?;
    reopened.apply_project(&persisted)?;
    let shape = reopened
        .authored_shape(node_id)
        .ok_or("authored shape missing after fresh project replay")?;
    if shape.page_id != page_id || shape.bounds != bounds || shape.paint != paint {
        return Err("authored shape changed across project replay".into());
    }

    let label = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source.pub");
    let idml = export_target(
        &reopened,
        EditorEditableTarget::Idml,
        label,
        &out,
        node_id,
    )?;
    let odg = export_target(
        &reopened,
        EditorEditableTarget::Odg,
        label,
        &out,
        node_id,
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "chaptera.authored-shape-export-probe.v1",
            "source_sha256": hash.to_string(),
            "node_id": node_id.as_canonical().to_string(),
            "page_id": page_id.as_canonical().to_string(),
            "bounds": {
                "x": bounds.x.get(),
                "y": bounds.y.get(),
                "width": bounds.width.get(),
                "height": bounds.height.get(),
            },
            "fill_rgb": [0x12, 0x34, 0x56],
            "stroke_rgb": [0xaa, 0xbb, 0xcc],
            "stroke_width_emu": 12_700,
            "project_replay": true,
            "idml": idml,
            "odg": odg,
        }))?
    );
    Ok(())
}
