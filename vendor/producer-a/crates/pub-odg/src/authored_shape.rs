use crate::{ODG_CONTENT_PATH, OdgPackage, OdgPartKind};
use pub_export::{
    AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE, AUTHORED_SHAPE_STROKE_FEATURE,
    AUTHORED_SHAPE_Z_ORDER_FEATURE, AuthoredRectangleExportV1, CapabilityLevel, ExportPlan,
    ExportSrgb8V1,
};
use pub_model::{CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, PageId};
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgAuthoredShapeError {
    NonOdgPackage,
    MissingContent,
    InvalidContentUtf8,
    DuplicateRectangle {
        node_id: NodeId,
    },
    InvalidBounds {
        node_id: NodeId,
    },
    InvalidStroke {
        node_id: NodeId,
    },
    MissingPreservedFeature {
        node_id: NodeId,
        feature: &'static str,
    },
    MissingZOrderLoss {
        node_id: NodeId,
    },
    MissingAutomaticStyles,
    MissingPage {
        page_id: PageId,
    },
}

impl fmt::Display for OdgAuthoredShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonOdgPackage => f.write_str("authored rectangle projection requires ODG"),
            Self::MissingContent => f.write_str("ODG content.xml missing"),
            Self::InvalidContentUtf8 => f.write_str("ODG content.xml is not UTF-8"),
            Self::DuplicateRectangle { node_id } => {
                write!(f, "duplicate authored rectangle {node_id}")
            }
            Self::InvalidBounds { node_id } => {
                write!(f, "authored rectangle {node_id} has invalid bounds")
            }
            Self::InvalidStroke { node_id } => {
                write!(f, "authored rectangle {node_id} has invalid stroke")
            }
            Self::MissingPreservedFeature { node_id, feature } => write!(
                f,
                "authored rectangle {node_id} lacks preserved {feature} in ExportPlan"
            ),
            Self::MissingZOrderLoss { node_id } => write!(
                f,
                "authored rectangle {node_id} lacks explicit authored_shape.z_order loss"
            ),
            Self::MissingAutomaticStyles => f.write_str("ODG automatic-styles container missing"),
            Self::MissingPage { page_id } => write!(f, "ODG page {page_id} missing"),
        }
    }
}

impl std::error::Error for OdgAuthoredShapeError {}

pub fn add_authored_rectangles_to_odg(
    plan: &ExportPlan,
    package: &mut OdgPackage,
    rectangles: &[AuthoredRectangleExportV1],
) -> Result<(), OdgAuthoredShapeError> {
    if plan.target.format != "odg" || package.target.format != "odg" {
        return Err(OdgAuthoredShapeError::NonOdgPackage);
    }
    let content_index = package
        .parts
        .iter()
        .position(|part| part.kind == OdgPartKind::Content && part.path == ODG_CONTENT_PATH)
        .ok_or(OdgAuthoredShapeError::MissingContent)?;
    let mut xml = String::from_utf8(package.parts[content_index].content.clone())
        .map_err(|_| OdgAuthoredShapeError::InvalidContentUtf8)?;

    let mut ordered = rectangles.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|item| (item.page_id, item.node_id));
    let mut seen = BTreeSet::new();
    for item in &ordered {
        if !seen.insert(item.node_id) {
            return Err(OdgAuthoredShapeError::DuplicateRectangle {
                node_id: item.node_id,
            });
        }
        validate(plan, item)?;
    }

    if ordered.is_empty() {
        return Ok(());
    }

    let mut styles = String::new();
    for item in &ordered {
        let style_name = style_name(item.node_id);
        write!(
            styles,
            "    <style:style style:name=\"{style_name}\" style:family=\"graphic\"><style:graphic-properties"
        )
        .unwrap();
        if item.fill.visible {
            write!(
                styles,
                " draw:fill=\"solid\" draw:fill-color=\"{}\"",
                hex_color(item.fill.color)
            )
            .unwrap();
        } else {
            styles.push_str(" draw:fill=\"none\"");
        }
        if item.stroke.visible {
            write!(
                styles,
                " draw:stroke=\"solid\" svg:stroke-color=\"{}\" svg:stroke-width=\"{}pt\"",
                hex_color(item.stroke.color),
                format_emu_points(item.stroke.width_emu)
            )
            .unwrap();
        } else {
            styles.push_str(" draw:stroke=\"none\"");
        }
        styles.push_str("/></style:style>\n");
    }
    insert_automatic_styles(&mut xml, &styles)?;

    for item in ordered {
        let page_marker = format!("<draw:page draw:name=\"{}\"", page_name(item.page_id));
        let Some(page_start) = xml.find(&page_marker) else {
            return Err(OdgAuthoredShapeError::MissingPage {
                page_id: item.page_id,
            });
        };
        let close = "      </draw:page>";
        let Some(relative_close) = xml[page_start..].find(close) else {
            return Err(OdgAuthoredShapeError::MissingPage {
                page_id: item.page_id,
            });
        };
        let insert_at = page_start + relative_close;
        let fragment = format!(
            "        <draw:rect draw:name=\"{}\" draw:style-name=\"{}\" svg:x=\"{}pt\" svg:y=\"{}pt\" svg:width=\"{}pt\" svg:height=\"{}pt\"/>\n",
            rect_name(item.node_id),
            style_name(item.node_id),
            format_emu_points(item.bounds.x),
            format_emu_points(item.bounds.y),
            format_emu_points(item.bounds.width),
            format_emu_points(item.bounds.height),
        );
        xml.insert_str(insert_at, &fragment);
    }

    package.parts[content_index].content = xml.into_bytes();
    Ok(())
}

fn validate(
    plan: &ExportPlan,
    item: &AuthoredRectangleExportV1,
) -> Result<(), OdgAuthoredShapeError> {
    if item.bounds.width.get() <= 0
        || item.bounds.height.get() <= 0
        || item.bounds.right().is_none()
        || item.bounds.bottom().is_none()
    {
        return Err(OdgAuthoredShapeError::InvalidBounds {
            node_id: item.node_id,
        });
    }
    if item.stroke.width_emu.get() <= 0 {
        return Err(OdgAuthoredShapeError::InvalidStroke {
            node_id: item.node_id,
        });
    }
    for feature in [
        AUTHORED_SHAPE_GEOMETRY_FEATURE,
        AUTHORED_SHAPE_FILL_FEATURE,
        AUTHORED_SHAPE_STROKE_FEATURE,
    ] {
        if !has_preserved_feature(plan, item.node_id.into_canonical(), feature) {
            return Err(OdgAuthoredShapeError::MissingPreservedFeature {
                node_id: item.node_id,
                feature,
            });
        }
    }
    if !has_reported_loss(
        plan,
        item.node_id.into_canonical(),
        AUTHORED_SHAPE_Z_ORDER_FEATURE,
    ) {
        return Err(OdgAuthoredShapeError::MissingZOrderLoss {
            node_id: item.node_id,
        });
    }
    Ok(())
}

fn insert_automatic_styles(xml: &mut String, styles: &str) -> Result<(), OdgAuthoredShapeError> {
    let empty = "  <office:automatic-styles/>\n";
    if xml.matches(empty).count() == 1 {
        let replacement =
            format!("  <office:automatic-styles>\n{styles}  </office:automatic-styles>\n");
        *xml = xml.replacen(empty, &replacement, 1);
        return Ok(());
    }
    let close = "  </office:automatic-styles>";
    let Some(index) = xml.find(close) else {
        return Err(OdgAuthoredShapeError::MissingAutomaticStyles);
    };
    xml.insert_str(index, styles);
    Ok(())
}

fn has_preserved_feature(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
    plan.features.iter().any(|planned| {
        planned.request.origin == Some(origin)
            && planned.request.feature == feature
            && planned.disposition == CapabilityLevel::Preserved
    })
}

fn has_reported_loss(plan: &ExportPlan, origin: CanonicalId, feature: &str) -> bool {
    plan.losses
        .iter()
        .any(|loss| loss.origin == Some(origin) && loss.feature == feature)
}

fn style_name(id: NodeId) -> String {
    stable_name("AuthoredRectStyle", id.into_canonical())
}

fn rect_name(id: NodeId) -> String {
    stable_name("AuthoredRect", id.into_canonical())
}

fn page_name(id: PageId) -> String {
    stable_name("Page", id.into_canonical())
}

fn stable_name(prefix: &str, id: CanonicalId) -> String {
    let mut value = String::with_capacity(prefix.len() + 33);
    value.push_str(prefix);
    value.push('_');
    for byte in id.into_bytes() {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

fn hex_color(color: ExportSrgb8V1) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn format_emu_points(value: LengthEmu) -> String {
    let numerator = i128::from(value.get());
    let denominator = i128::from(EMU_PER_POINT);
    let negative = numerator < 0;
    let numerator = numerator.abs();
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    write!(result, "{whole}").unwrap();
    if remainder == 0 {
        return result;
    }
    result.push('.');
    for _ in 0..15 {
        remainder *= 10;
        let digit = remainder / denominator;
        result.push(char::from(
            b'0' + u8::try_from(digit).expect("decimal digit"),
        ));
        remainder %= denominator;
        if remainder == 0 {
            break;
        }
    }
    while result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ODG_ADAPTER_VERSION_V0_1, ODG_SCHEMA_FENCE_ODF_1_4, OdgPart};
    use pub_export::{
        AUTHORED_SHAPE_FILL_FEATURE, AUTHORED_SHAPE_GEOMETRY_FEATURE,
        AUTHORED_SHAPE_STROKE_FEATURE, AUTHORED_SHAPE_Z_ORDER_FEATURE, ExportSolidPaintV1,
        ExportSolidStrokeV1, SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile,
        plan_export,
    };
    use pub_model::{CanonicalId, RectEmu, Size2D};
    use std::collections::BTreeMap;

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn rectangle() -> AuthoredRectangleExportV1 {
        AuthoredRectangleExportV1 {
            node_id: NodeId::from_canonical(id(2)),
            page_id: PageId::from_canonical(id(1)),
            page_size: Size2D::new(LengthEmu::new(7_620_000), LengthEmu::new(9_906_000)),
            bounds: RectEmu::new(
                LengthEmu::new(1_270_000),
                LengthEmu::new(2_540_000),
                LengthEmu::new(2_540_000),
                LengthEmu::new(1_270_000),
            ),
            fill: ExportSolidPaintV1 {
                visible: true,
                color: ExportSrgb8V1 {
                    r: 0x12,
                    g: 0x34,
                    b: 0x56,
                },
            },
            stroke: ExportSolidStrokeV1 {
                visible: true,
                color: ExportSrgb8V1 {
                    r: 0xAA,
                    g: 0xBB,
                    b: 0xCC,
                },
                width_emu: LengthEmu::new(12_700),
            },
        }
    }

    fn plan(item: &AuthoredRectangleExportV1) -> ExportPlan {
        let mut features = BTreeMap::new();
        for feature in [
            AUTHORED_SHAPE_GEOMETRY_FEATURE,
            AUTHORED_SHAPE_FILL_FEATURE,
            AUTHORED_SHAPE_STROKE_FEATURE,
        ] {
            features.insert(feature.into(), CapabilityLevel::Preserved);
        }
        let manifest = TargetCapabilityManifest {
            target: TargetProfile {
                format: "odg".into(),
                adapter_version: ODG_ADAPTER_VERSION_V0_1.into(),
                profile: "bounded-editable".into(),
                schema_fence: Some(ODG_SCHEMA_FENCE_ODF_1_4.into()),
            },
            features,
        };
        let request = |feature: &str, required| SemanticFeatureRequest {
            feature: feature.into(),
            origin: Some(item.node_id.into_canonical()),
            property_path: None,
            require_preserved: required,
        };
        plan_export(
            &manifest,
            vec![
                request(AUTHORED_SHAPE_GEOMETRY_FEATURE, true),
                request(AUTHORED_SHAPE_FILL_FEATURE, true),
                request(AUTHORED_SHAPE_STROKE_FEATURE, true),
                request(AUTHORED_SHAPE_Z_ORDER_FEATURE, false),
            ],
        )
    }

    fn package(item: &AuthoredRectangleExportV1, plan: &ExportPlan) -> OdgPackage {
        let content = format!(
            "<?xml version=\"1.0\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\">\n  <office:automatic-styles/>\n  <office:body><office:drawing><draw:page draw:name=\"{}\">\n      </draw:page></office:drawing></office:body></office:document-content>",
            page_name(item.page_id)
        );
        OdgPackage {
            target: plan.target.clone(),
            conversion_fence: None,
            parts: vec![OdgPart {
                path: ODG_CONTENT_PATH.into(),
                kind: OdgPartKind::Content,
                media_type: "text/xml".into(),
                content: content.into_bytes(),
            }],
        }
    }

    #[test]
    fn writes_native_graphic_style_rectangle_and_no_fake_z_index() {
        let item = rectangle();
        let plan = plan(&item);
        assert!(plan.losses.iter().any(|loss| {
            loss.origin == Some(item.node_id.into_canonical())
                && loss.feature == AUTHORED_SHAPE_Z_ORDER_FEATURE
        }));
        let mut package = package(&item, &plan);
        add_authored_rectangles_to_odg(&plan, &mut package, std::slice::from_ref(&item))
            .expect("authored rectangle");
        let xml = std::str::from_utf8(&package.parts[0].content).unwrap();
        assert!(xml.contains("style:family=\"graphic\""));
        assert!(xml.contains("draw:fill=\"solid\" draw:fill-color=\"#123456\""));
        assert!(xml.contains(
            "draw:stroke=\"solid\" svg:stroke-color=\"#AABBCC\" svg:stroke-width=\"1pt\""
        ));
        assert!(xml.contains("<draw:rect draw:name=\"AuthoredRect_"));
        assert!(
            xml.contains(
                "svg:x=\"100pt\" svg:y=\"200pt\" svg:width=\"200pt\" svg:height=\"100pt\""
            )
        );
        assert!(!xml.contains("draw:z-index="));
    }

    #[test]
    fn invisible_fill_and_stroke_remain_explicit_none() {
        let mut item = rectangle();
        item.fill.visible = false;
        item.stroke.visible = false;
        let plan = plan(&item);
        let mut package = package(&item, &plan);
        add_authored_rectangles_to_odg(&plan, &mut package, std::slice::from_ref(&item))
            .expect("authored rectangle");
        let xml = std::str::from_utf8(&package.parts[0].content).unwrap();
        assert!(xml.contains("draw:fill=\"none\""));
        assert!(xml.contains("draw:stroke=\"none\""));
    }
}
