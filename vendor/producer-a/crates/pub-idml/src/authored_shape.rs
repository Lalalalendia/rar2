use crate::{IDML_PACKAGING_NAMESPACE, IdmlPackage, IdmlPart, IdmlPartContent, IdmlPartKind};
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
pub enum IdmlAuthoredShapeError {
    NonIdmlPackage,
    DuplicateRectangle {
        node_id: NodeId,
    },
    InvalidPageSize {
        page_id: PageId,
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
    MissingSpread {
        page_id: PageId,
    },
    BinarySpread {
        page_id: PageId,
    },
    InvalidSpreadXml {
        page_id: PageId,
    },
    MissingDesignMap,
    BinaryDesignMap,
    UnexpectedDesignMapMarkup,
    DuplicateGraphicResource,
}

impl fmt::Display for IdmlAuthoredShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonIdmlPackage => f.write_str("authored rectangle projection requires IDML"),
            Self::DuplicateRectangle { node_id } => {
                write!(f, "duplicate authored rectangle {node_id}")
            }
            Self::InvalidPageSize { page_id } => {
                write!(f, "authored rectangle page {page_id} has invalid size")
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
            Self::MissingSpread { page_id } => write!(f, "IDML spread missing for page {page_id}"),
            Self::BinarySpread { page_id } => write!(f, "IDML spread for page {page_id} is binary"),
            Self::InvalidSpreadXml { page_id } => write!(
                f,
                "IDML spread for page {page_id} is not overlay-compatible"
            ),
            Self::MissingDesignMap => f.write_str("IDML designmap.xml missing"),
            Self::BinaryDesignMap => f.write_str("IDML designmap.xml is binary"),
            Self::UnexpectedDesignMapMarkup => {
                f.write_str("IDML designmap Document root is not overlay-compatible")
            }
            Self::DuplicateGraphicResource => f.write_str(
                "IDML already carries Resources/Graphic.xml outside authored-shape overlay",
            ),
        }
    }
}

impl std::error::Error for IdmlAuthoredShapeError {}

pub fn add_authored_rectangles_to_idml(
    plan: &ExportPlan,
    package: &mut IdmlPackage,
    rectangles: &[AuthoredRectangleExportV1],
) -> Result<(), IdmlAuthoredShapeError> {
    if plan.target.format != "idml" || package.target.format != "idml" {
        return Err(IdmlAuthoredShapeError::NonIdmlPackage);
    }

    let mut ordered = rectangles.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|item| (item.page_id, item.node_id));
    let mut seen = BTreeSet::new();
    let mut colors = BTreeSet::new();

    for item in &ordered {
        if !seen.insert(item.node_id) {
            return Err(IdmlAuthoredShapeError::DuplicateRectangle {
                node_id: item.node_id,
            });
        }
        validate(plan, item)?;
        if item.fill.visible {
            colors.insert(item.fill.color);
        }
        if item.stroke.visible {
            colors.insert(item.stroke.color);
        }
    }

    if !colors.is_empty() {
        add_graphic_resource(package, &colors)?;
    }

    for item in ordered {
        let spread_path = spread_path(item.page_id);
        let spread = package
            .parts
            .iter_mut()
            .find(|part| part.kind == IdmlPartKind::Spread && part.path == spread_path)
            .ok_or(IdmlAuthoredShapeError::MissingSpread {
                page_id: item.page_id,
            })?;
        let IdmlPartContent::Text(xml) = &mut spread.content else {
            return Err(IdmlAuthoredShapeError::BinarySpread {
                page_id: item.page_id,
            });
        };
        let marker = "  </Spread>";
        let Some(insert_at) = xml.rfind(marker) else {
            return Err(IdmlAuthoredShapeError::InvalidSpreadXml {
                page_id: item.page_id,
            });
        };
        xml.insert_str(insert_at, &rectangle_xml(item)?);
    }
    Ok(())
}

fn validate(
    plan: &ExportPlan,
    item: &AuthoredRectangleExportV1,
) -> Result<(), IdmlAuthoredShapeError> {
    if !item.page_size.is_positive() {
        return Err(IdmlAuthoredShapeError::InvalidPageSize {
            page_id: item.page_id,
        });
    }
    if item.bounds.width.get() <= 0
        || item.bounds.height.get() <= 0
        || item.bounds.right().is_none()
        || item.bounds.bottom().is_none()
    {
        return Err(IdmlAuthoredShapeError::InvalidBounds {
            node_id: item.node_id,
        });
    }
    if item.stroke.width_emu.get() <= 0 {
        return Err(IdmlAuthoredShapeError::InvalidStroke {
            node_id: item.node_id,
        });
    }
    for feature in [
        AUTHORED_SHAPE_GEOMETRY_FEATURE,
        AUTHORED_SHAPE_FILL_FEATURE,
        AUTHORED_SHAPE_STROKE_FEATURE,
    ] {
        if !has_preserved_feature(plan, item.node_id.into_canonical(), feature) {
            return Err(IdmlAuthoredShapeError::MissingPreservedFeature {
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
        return Err(IdmlAuthoredShapeError::MissingZOrderLoss {
            node_id: item.node_id,
        });
    }
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

fn add_graphic_resource(
    package: &mut IdmlPackage,
    colors: &BTreeSet<ExportSrgb8V1>,
) -> Result<(), IdmlAuthoredShapeError> {
    if package
        .parts
        .iter()
        .any(|part| part.path == "Resources/Graphic.xml")
    {
        return Err(IdmlAuthoredShapeError::DuplicateGraphicResource);
    }
    let designmap = package
        .parts
        .iter_mut()
        .find(|part| part.kind == IdmlPartKind::DesignMap && part.path == "designmap.xml")
        .ok_or(IdmlAuthoredShapeError::MissingDesignMap)?;
    let IdmlPartContent::Text(xml) = &mut designmap.content else {
        return Err(IdmlAuthoredShapeError::BinaryDesignMap);
    };
    if xml.contains("<idPkg:Graphic") {
        return Err(IdmlAuthoredShapeError::DuplicateGraphicResource);
    }
    let document_start = xml
        .find("<Document")
        .ok_or(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup)?;
    let open_end = xml[document_start..]
        .find('>')
        .map(|relative| document_start + relative)
        .ok_or(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup)?;
    if xml[..=open_end].ends_with("/>") {
        return Err(IdmlAuthoredShapeError::UnexpectedDesignMapMarkup);
    }
    xml.insert_str(
        open_end + 1,
        "\n  <idPkg:Graphic src=\"Resources/Graphic.xml\"/>",
    );

    let mut graphic = String::new();
    writeln!(graphic, "<?xml version=\"1.0\" encoding=\"utf-8\"?>").unwrap();
    writeln!(
        graphic,
        "<idPkg:Graphic xmlns:idPkg=\"{}\" DOMVersion=\"7.0\">",
        IDML_PACKAGING_NAMESPACE
    )
    .unwrap();
    for color in colors {
        let id = color_id(*color);
        writeln!(
            graphic,
            "  <Color Self=\"{id}\" Model=\"Process\" Space=\"RGB\" ColorValue=\"{} {} {}\" ColorOverride=\"Normal\" Name=\"{}\" ColorEditable=\"true\" ColorRemovable=\"true\" Visible=\"true\"/>",
            color.r,
            color.g,
            color.b,
            color_name(*color),
        )
        .unwrap();
    }
    graphic.push_str("</idPkg:Graphic>\n");
    package.parts.push(IdmlPart {
        path: "Resources/Graphic.xml".into(),
        kind: IdmlPartKind::Resource,
        content: IdmlPartContent::Text(graphic),
    });
    package.parts.sort();
    Ok(())
}

fn rectangle_xml(item: &AuthoredRectangleExportV1) -> Result<String, IdmlAuthoredShapeError> {
    let right = item
        .bounds
        .right()
        .ok_or(IdmlAuthoredShapeError::InvalidBounds {
            node_id: item.node_id,
        })?;
    let bottom = item
        .bounds
        .bottom()
        .ok_or(IdmlAuthoredShapeError::InvalidBounds {
            node_id: item.node_id,
        })?;

    let x = format_emu_points(item.bounds.x);
    let y = format_emu_points(item.bounds.y);
    let right = format_emu_points(right);
    let bottom = format_emu_points(bottom);
    let tx = format_ratio(
        -i128::from(item.page_size.width.get()),
        i128::from(EMU_PER_POINT),
        15,
    );
    let ty = format_ratio(
        -i128::from(item.page_size.height.get()),
        i128::from(EMU_PER_POINT) * 2,
        15,
    );
    let fill = if item.fill.visible {
        color_id(item.fill.color)
    } else {
        "Swatch/None".into()
    };
    let stroke = if item.stroke.visible {
        color_id(item.stroke.color)
    } else {
        "Swatch/None".into()
    };
    let stroke_weight = if item.stroke.visible {
        format_emu_points(item.stroke.width_emu)
    } else {
        "0".into()
    };

    let mut xml = String::new();
    writeln!(
        xml,
        "    <Rectangle Self=\"{}\" ContentType=\"GraphicType\" AppliedObjectStyle=\"ObjectStyle/$ID/[None]\" Visible=\"true\" ItemTransform=\"1 0 0 1 {tx} {ty}\" FillColor=\"{fill}\" StrokeColor=\"{stroke}\" StrokeWeight=\"{stroke_weight}\">",
        idml_self("uar", item.node_id.into_canonical()),
    )
    .unwrap();
    xml.push_str("      <Properties>\n");
    xml.push_str("        <PathGeometry>\n");
    xml.push_str("          <GeometryPathType PathOpen=\"false\">\n");
    xml.push_str("            <PathPointArray>\n");
    write_path_point(&mut xml, &x, &y);
    write_path_point(&mut xml, &x, &bottom);
    write_path_point(&mut xml, &right, &bottom);
    write_path_point(&mut xml, &right, &y);
    xml.push_str("            </PathPointArray>\n");
    xml.push_str("          </GeometryPathType>\n");
    xml.push_str("        </PathGeometry>\n");
    xml.push_str("      </Properties>\n");
    xml.push_str("    </Rectangle>\n");
    Ok(xml)
}

fn color_name(color: ExportSrgb8V1) -> String {
    format!("Chaptera-{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn color_id(color: ExportSrgb8V1) -> String {
    format!("Color/{}", color_name(color))
}

fn write_path_point(xml: &mut String, x: &str, y: &str) {
    writeln!(
        xml,
        "              <PathPointType Anchor=\"{x} {y}\" LeftDirection=\"{x} {y}\" RightDirection=\"{x} {y}\"/>"
    )
    .unwrap();
}

fn spread_path(page_id: PageId) -> String {
    format!(
        "Spreads/Spread_{}.xml",
        idml_self("usp", page_id.into_canonical())
    )
}

fn idml_self(prefix: &str, id: CanonicalId) -> String {
    let mut value = String::with_capacity(prefix.len() + 32);
    value.push_str(prefix);
    for byte in id.into_bytes() {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

fn format_emu_points(value: LengthEmu) -> String {
    format_ratio(i128::from(value.get()), i128::from(EMU_PER_POINT), 15)
}

fn format_ratio(numerator: i128, denominator: i128, precision: usize) -> String {
    debug_assert!(denominator > 0);
    if numerator == 0 {
        return "0".into();
    }
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
    for _ in 0..precision {
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
    use crate::{IDML_ADAPTER_VERSION_V0_1, IdmlPackageBuilder};
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
                format: "idml".into(),
                adapter_version: IDML_ADAPTER_VERSION_V0_1.into(),
                profile: "bounded-editable".into(),
                schema_fence: Some("legacy-spec-8.02/dom-7.0".into()),
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

    fn package(item: &AuthoredRectangleExportV1, plan: &ExportPlan) -> IdmlPackage {
        let mut builder = IdmlPackageBuilder::from_export_plan(plan).unwrap();
        builder
            .add_part(
                "designmap.xml",
                IdmlPartKind::DesignMap,
                format!(
                    "<Document xmlns:idPkg=\"{}\" DOMVersion=\"7.0\">\n</Document>\n",
                    IDML_PACKAGING_NAMESPACE
                ),
            )
            .unwrap();
        builder
            .add_part(
                spread_path(item.page_id),
                IdmlPartKind::Spread,
                format!(
                    "<idPkg:Spread xmlns:idPkg=\"{}\"><Spread Self=\"x\">\n  </Spread></idPkg:Spread>\n",
                    IDML_PACKAGING_NAMESPACE
                ),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[test]
    fn writes_rgb_resources_rectangle_geometry_and_explicit_z_order_loss() {
        let item = rectangle();
        let plan = plan(&item);
        assert!(plan.losses.iter().any(|loss| {
            loss.origin == Some(item.node_id.into_canonical())
                && loss.feature == AUTHORED_SHAPE_Z_ORDER_FEATURE
        }));
        let mut package = package(&item, &plan);
        add_authored_rectangles_to_idml(&plan, &mut package, std::slice::from_ref(&item))
            .expect("authored rectangle");

        let designmap = package
            .parts
            .iter()
            .find(|part| part.path == "designmap.xml")
            .and_then(|part| part.content.as_text())
            .unwrap();
        assert!(designmap.contains("<idPkg:Graphic src=\"Resources/Graphic.xml\"/>"));

        let graphic = package
            .parts
            .iter()
            .find(|part| part.path == "Resources/Graphic.xml")
            .and_then(|part| part.content.as_text())
            .unwrap();
        assert!(graphic.contains("Space=\"RGB\" ColorValue=\"18 52 86\""));
        assert!(graphic.contains("Space=\"RGB\" ColorValue=\"170 187 204\""));

        let spread = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Spread)
            .and_then(|part| part.content.as_text())
            .unwrap();
        assert!(spread.contains("<Rectangle Self=\"uar"));
        assert!(spread.contains("FillColor=\"Color/Chaptera-123456\""));
        assert!(spread.contains("StrokeColor=\"Color/Chaptera-AABBCC\""));
        assert!(spread.contains("StrokeWeight=\"1\""));
        assert!(spread.contains("Anchor=\"100 200\""));
        assert!(spread.contains("Anchor=\"300 300\""));
    }

    #[test]
    fn invisible_paint_uses_none_without_fake_color_resource() {
        let mut item = rectangle();
        item.fill.visible = false;
        item.stroke.visible = false;
        let plan = plan(&item);
        let mut package = package(&item, &plan);
        add_authored_rectangles_to_idml(&plan, &mut package, std::slice::from_ref(&item))
            .expect("authored rectangle");
        assert!(
            package
                .parts
                .iter()
                .all(|part| part.path != "Resources/Graphic.xml")
        );
        let spread = package
            .parts
            .iter()
            .find(|part| part.kind == IdmlPartKind::Spread)
            .and_then(|part| part.content.as_text())
            .unwrap();
        assert!(spread.contains("FillColor=\"Swatch/None\""));
        assert!(spread.contains("StrokeColor=\"Swatch/None\""));
        assert!(spread.contains("StrokeWeight=\"0\""));
    }
}
