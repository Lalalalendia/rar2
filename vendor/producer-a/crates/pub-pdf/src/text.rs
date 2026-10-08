use pub_layout::{BoundedShapedText, font_fingerprint_sha256};
use pub_model::{LengthEmu, NodeId};
use pub_output::{
    FontIdentity, OutputFontDisposition, OutputFontMaterialization, OutputFontPlan,
    PlannedOutputFont,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedFontResource {
    pub identity: FontIdentity,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedTextRun {
    pub node_id: NodeId,
    /// Story-global Unicode-scalar index corresponding to logical_text[0].
    ///
    /// Existing single-run callers default to zero. Shaped-flow lines retain
    /// their Story-global glyph clusters and set this to line.scalar_start so
    /// PDF text slicing can map clusters locally without rewriting provenance.
    #[serde(default)]
    pub scalar_base: u32,
    pub logical_text: String,
    pub shaped: BoundedShapedText,
    pub baseline_x: LengthEmu,
    pub baseline_y: LengthEmu,
    pub fill_rgb: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfTextPreparationError {
    TextReferencesMissingNode {
        node_id: NodeId,
    },
    MissingFontPlan,
    BlockedFontPlan,
    DuplicateFontResource {
        identity: FontIdentity,
    },
    MissingFontPlanEntry {
        identity: FontIdentity,
    },
    MissingFontResource {
        identity: FontIdentity,
    },
    FontFingerprintMismatch {
        identity: FontIdentity,
        actual: String,
    },
    PlannedGlyphMissing {
        identity: FontIdentity,
        glyph_id: u32,
    },
    ConflictingToUnicode {
        identity: FontIdentity,
        glyph_id: u16,
    },
    ClusterBeforeScalarBase {
        node_id: NodeId,
        cluster: u32,
        scalar_base: u32,
    },
    ClusterOutsideRun {
        node_id: NodeId,
        cluster: u32,
        scalar_base: u32,
        logical_scalar_len: usize,
    },
}

impl fmt::Display for PdfTextPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextReferencesMissingNode { node_id } => {
                write!(formatter, "resolved text references missing node {node_id:?}")
            }
            Self::MissingFontPlan => formatter.write_str(
                "resolved text requires an explicit OutputFontPlan; renderer will not invent font policy",
            ),
            Self::BlockedFontPlan => formatter.write_str(
                "resolved text OutputFontPlan contains blocking diagnostics",
            ),
            Self::DuplicateFontResource { identity } => {
                write!(formatter, "duplicate fixed font resource for {identity:?}")
            }
            Self::MissingFontPlanEntry { identity } => {
                write!(formatter, "no OutputFontPlan entry for resolved font {identity:?}")
            }
            Self::MissingFontResource { identity } => {
                write!(formatter, "no exact fixed font bytes for resolved font {identity:?}")
            }
            Self::FontFingerprintMismatch { identity, actual } => write!(
                formatter,
                "fixed font bytes do not match planned identity {identity:?}; actual SHA-256 is {actual}"
            ),
            Self::PlannedGlyphMissing { identity, glyph_id } => write!(
                formatter,
                "resolved glyph {glyph_id} was not present in OutputFontPlan for {identity:?}"
            ),
            Self::ConflictingToUnicode { identity, glyph_id } => write!(
                formatter,
                "glyph {glyph_id} has conflicting Unicode mappings for {identity:?}"
            ),
            Self::ClusterBeforeScalarBase {
                node_id,
                cluster,
                scalar_base,
            } => write!(
                formatter,
                "resolved text node {node_id:?} glyph cluster {cluster} precedes scalar_base {scalar_base}"
            ),
            Self::ClusterOutsideRun {
                node_id,
                cluster,
                scalar_base,
                logical_scalar_len,
            } => write!(
                formatter,
                "resolved text node {node_id:?} glyph cluster {cluster} is outside run scalar_base {scalar_base} with logical scalar length {logical_scalar_len}"
            ),
        }
    }
}

impl std::error::Error for PdfTextPreparationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedFont {
    pub identity: FontIdentity,
    pub bytes: Vec<u8>,
    pub base_name: String,
    pub to_unicode: BTreeMap<u16, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedGlyph {
    pub glyph_id: u16,
    pub x_emu: i64,
    pub y_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedTextRun {
    Painted {
        font_identity: FontIdentity,
        font_size_emu: i64,
        baseline_x_emu: i64,
        baseline_y_emu: i64,
        fill_rgb: [u8; 3],
        actual_text: String,
        glyphs: Vec<PreparedGlyph>,
    },
    Unsupported {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct PreparedTextResources {
    pub fonts: BTreeMap<FontIdentity, PreparedFont>,
    pub runs_by_node: BTreeMap<NodeId, Vec<PreparedTextRun>>,
}

pub(crate) fn prepare_text_resources(
    font_plan: Option<&OutputFontPlan>,
    font_resources: &[FixedFontResource],
    text_runs: &[FixedTextRun],
    node_ids: &BTreeSet<NodeId>,
) -> Result<PreparedTextResources, PdfTextPreparationError> {
    if text_runs.is_empty() {
        return Ok(PreparedTextResources::default());
    }

    let plan = font_plan.ok_or(PdfTextPreparationError::MissingFontPlan)?;
    if !plan.can_serialize() {
        return Err(PdfTextPreparationError::BlockedFontPlan);
    }

    let mut exact_fonts = BTreeMap::<FontIdentity, &FixedFontResource>::new();
    for resource in font_resources {
        if exact_fonts
            .insert(resource.identity.clone(), resource)
            .is_some()
        {
            return Err(PdfTextPreparationError::DuplicateFontResource {
                identity: resource.identity.clone(),
            });
        }
    }

    let planned = plan
        .fonts
        .iter()
        .map(|font| (font.source.clone(), font))
        .collect::<BTreeMap<_, _>>();

    let mut prepared = PreparedTextResources::default();

    for run in text_runs {
        if !node_ids.contains(&run.node_id) {
            return Err(PdfTextPreparationError::TextReferencesMissingNode {
                node_id: run.node_id,
            });
        }

        let identity = FontIdentity {
            fingerprint_sha256: run.shaped.environment.layout.font_set_fingerprint.clone(),
            face_index: run.shaped.environment.face_index,
        };
        let planned_font = planned.get(&identity).copied().ok_or_else(|| {
            PdfTextPreparationError::MissingFontPlanEntry {
                identity: identity.clone(),
            }
        })?;

        let unsupported = unsupported_plan_reason(planned_font);
        if let Some((code, message)) = unsupported {
            prepared.runs_by_node.entry(run.node_id).or_default().push(
                PreparedTextRun::Unsupported {
                    code: code.into(),
                    message: message.into(),
                },
            );
            continue;
        }

        let resource = exact_fonts.get(&identity).copied().ok_or_else(|| {
            PdfTextPreparationError::MissingFontResource {
                identity: identity.clone(),
            }
        })?;
        let actual = font_fingerprint_sha256(&resource.bytes);
        if actual != identity.fingerprint_sha256 {
            return Err(PdfTextPreparationError::FontFingerprintMismatch {
                identity: identity.clone(),
                actual,
            });
        }

        if identity.face_index != 0 || !is_standalone_truetype(&resource.bytes) {
            prepared
                .runs_by_node
                .entry(run.node_id)
                .or_default()
                .push(PreparedTextRun::Unsupported {
                code: "pdf.text.font_program_unsupported".into(),
                message:
                    "bounded text PDF v0.1 supports only face 0 of standalone TrueType glyf fonts"
                        .into(),
            });
            continue;
        }

        for glyph in &run.shaped.glyphs {
            if planned_font
                .used_glyph_ids
                .binary_search(&glyph.glyph_id)
                .is_err()
            {
                return Err(PdfTextPreparationError::PlannedGlyphMissing {
                    identity: identity.clone(),
                    glyph_id: glyph.glyph_id,
                });
            }
        }

        let Some((glyphs, mappings)) = prepare_run_glyphs(run)? else {
            prepared
                .runs_by_node
                .entry(run.node_id)
                .or_default()
                .push(PreparedTextRun::Unsupported {
                    code: "pdf.text.cluster_mapping_unsupported".into(),
                    message: "resolved glyph clusters cannot be represented by the bounded one-CID-to-one-Unicode-sequence mapping"
                        .into(),
                });
            continue;
        };

        let font = prepared
            .fonts
            .entry(identity.clone())
            .or_insert_with(|| PreparedFont {
                identity: identity.clone(),
                bytes: resource.bytes.clone(),
                base_name: base_font_name(&identity),
                to_unicode: BTreeMap::new(),
            });

        for (glyph_id, text) in mappings {
            if let Some(existing) = font.to_unicode.get(&glyph_id) {
                if existing != &text {
                    return Err(PdfTextPreparationError::ConflictingToUnicode {
                        identity: identity.clone(),
                        glyph_id,
                    });
                }
            } else {
                font.to_unicode.insert(glyph_id, text);
            }
        }

        prepared
            .runs_by_node
            .entry(run.node_id)
            .or_default()
            .push(PreparedTextRun::Painted {
                font_identity: identity,
                font_size_emu: run.shaped.environment.font_size_emu.get(),
                baseline_x_emu: run.baseline_x.get(),
                baseline_y_emu: run.baseline_y.get(),
                fill_rgb: run.fill_rgb,
                actual_text: run.logical_text.clone(),
                glyphs,
            });
    }

    Ok(prepared)
}

fn unsupported_plan_reason(font: &PlannedOutputFont) -> Option<(&'static str, &'static str)> {
    if font.disposition != OutputFontDisposition::EmbedFull {
        return Some((
            "pdf.text.font_disposition_unsupported",
            "bounded text PDF v0.1 currently serializes only OutputFontPlan embed-full",
        ));
    }

    let Some(chosen) = font.chosen_output.as_ref() else {
        return Some((
            "pdf.text.font_materialization_missing",
            "embed-full plan has no chosen output font materialization",
        ));
    };
    if chosen.identity != font.source
        || chosen.materialization != OutputFontMaterialization::EmbedFull
    {
        return Some((
            "pdf.text.font_materialization_unsupported",
            "bounded text PDF v0.1 requires embed-full of the resolved source font identity",
        ));
    }
    None
}

fn prepare_run_glyphs(
    run: &FixedTextRun,
) -> Result<Option<(Vec<PreparedGlyph>, BTreeMap<u16, String>)>, PdfTextPreparationError> {
    let chars = run.logical_text.chars().collect::<Vec<_>>();
    if run.shaped.glyphs.is_empty() {
        return Ok(Some((Vec::new(), BTreeMap::new())));
    }

    let mut clusters = Vec::with_capacity(run.shaped.glyphs.len());
    for glyph in &run.shaped.glyphs {
        let local = glyph.cluster.checked_sub(run.scalar_base).ok_or(
            PdfTextPreparationError::ClusterBeforeScalarBase {
                node_id: run.node_id,
                cluster: glyph.cluster,
                scalar_base: run.scalar_base,
            },
        )?;
        let Ok(local) = usize::try_from(local) else {
            return Ok(None);
        };
        if local >= chars.len() {
            return Err(PdfTextPreparationError::ClusterOutsideRun {
                node_id: run.node_id,
                cluster: glyph.cluster,
                scalar_base: run.scalar_base,
                logical_scalar_len: chars.len(),
            });
        }
        clusters.push(local);
    }

    let mut unique = clusters.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != clusters.len() {
        return Ok(None);
    }

    let mut next_cluster = BTreeMap::new();
    for (index, cluster) in unique.iter().copied().enumerate() {
        next_cluster.insert(
            cluster,
            unique.get(index + 1).copied().unwrap_or(chars.len()),
        );
    }

    let mut glyphs = Vec::with_capacity(run.shaped.glyphs.len());
    let mut mappings = BTreeMap::<u16, String>::new();
    let mut pen_x = 0_i64;
    let mut pen_y = 0_i64;

    for (index, glyph) in run.shaped.glyphs.iter().enumerate() {
        let Ok(glyph_id) = u16::try_from(glyph.glyph_id) else {
            return Ok(None);
        };
        let cluster = clusters[index];
        let Some(&end) = next_cluster.get(&cluster) else {
            return Ok(None);
        };
        if end <= cluster || end > chars.len() {
            return Ok(None);
        }
        let text = chars[cluster..end].iter().collect::<String>();

        if let Some(existing) = mappings.get(&glyph_id) {
            if existing != &text {
                return Ok(None);
            }
        } else {
            mappings.insert(glyph_id, text);
        }

        let Some(x_emu) = pen_x.checked_add(glyph.x_offset.get()) else {
            return Ok(None);
        };
        let Some(y_emu) = pen_y.checked_sub(glyph.y_offset.get()) else {
            return Ok(None);
        };
        glyphs.push(PreparedGlyph {
            glyph_id,
            x_emu,
            y_emu,
        });
        let Some(next_pen_x) = pen_x.checked_add(glyph.x_advance.get()) else {
            return Ok(None);
        };
        let Some(next_pen_y) = pen_y.checked_sub(glyph.y_advance.get()) else {
            return Ok(None);
        };
        pen_x = next_pen_x;
        pen_y = next_pen_y;
    }

    Ok(Some((glyphs, mappings)))
}

fn is_standalone_truetype(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]) || bytes.starts_with(b"true")
}

fn base_font_name(identity: &FontIdentity) -> String {
    format!(
        "PUBF{}",
        identity
            .fingerprint_sha256
            .chars()
            .take(16)
            .collect::<String>()
            .to_ascii_uppercase()
    )
}

pub(crate) fn font_resource_name(identity: &FontIdentity) -> String {
    format!(
        "F{}",
        identity
            .fingerprint_sha256
            .chars()
            .take(12)
            .collect::<String>()
            .to_ascii_uppercase()
    )
}

pub(crate) fn append_text(
    content: &mut String,
    node_x_emu: i64,
    node_y_emu: i64,
    run: &PreparedTextRun,
) -> Option<FontIdentity> {
    let PreparedTextRun::Painted {
        font_identity,
        font_size_emu,
        baseline_x_emu,
        baseline_y_emu,
        fill_rgb,
        actual_text,
        glyphs,
    } = run
    else {
        return None;
    };

    content.push_str(&format!(
        "/Span << /ActualText <FEFF{}> >> BDC\n",
        utf16be_hex(actual_text)
    ));
    content.push_str("BT\n");
    content.push_str(&format!(
        "/{} {} Tf\n{} {} {} rg\n",
        font_resource_name(font_identity),
        super::format_points(*font_size_emu),
        super::format_rgb(fill_rgb[0]),
        super::format_rgb(fill_rgb[1]),
        super::format_rgb(fill_rgb[2]),
    ));

    for glyph in glyphs {
        let x = node_x_emu + *baseline_x_emu + glyph.x_emu;
        let y = node_y_emu + *baseline_y_emu + glyph.y_emu;
        content.push_str(&format!(
            "1 0 0 -1 {} {} Tm\n<{:04X}> Tj\n",
            super::format_points(x),
            super::format_points(y),
            glyph.glyph_id,
        ));
    }
    content.push_str("ET\nEMC\n");
    Some(font_identity.clone())
}

pub(crate) fn to_unicode_cmap(font: &PreparedFont) -> Vec<u8> {
    let mut body = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );

    let entries = font.to_unicode.iter().collect::<Vec<_>>();
    for chunk in entries.chunks(100) {
        body.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (glyph_id, text) in chunk {
            body.push_str(&format!("<{glyph_id:04X}> <{}>\n", utf16be_hex(text)));
        }
        body.push_str("endbfchar\n");
    }
    body.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    body.into_bytes()
}

fn utf16be_hex(text: &str) -> String {
    let mut output = String::new();
    for unit in text.encode_utf16() {
        output.push_str(&format!("{unit:04X}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_truetype_gate_is_bounded() {
        assert!(is_standalone_truetype(&[0, 1, 0, 0, 1]));
        assert!(is_standalone_truetype(b"true1234"));
        assert!(!is_standalone_truetype(b"OTTO1234"));
        assert!(!is_standalone_truetype(b"ttcf1234"));
    }

    #[test]
    fn story_global_clusters_use_scalar_base_only_for_local_text_slicing() {
        use pub_layout::{
            BoundedLayoutEnvironment, BoundedShapedGlyph, BoundedShapedText,
            BoundedShapingDescriptor,
        };
        use pub_model::{CanonicalId, LengthEmu};

        let node_id = NodeId::from_canonical(CanonicalId::from_bytes([7; 16]));
        let run = FixedTextRun {
            node_id,
            scalar_base: 2,
            logical_text: "BC".into(),
            shaped: BoundedShapedText {
                environment: BoundedShapingDescriptor {
                    layout: BoundedLayoutEnvironment {
                        engine_revision: "test".into(),
                        font_set_fingerprint: "a".repeat(64),
                        resource_fingerprint: "test".into(),
                    },
                    face_index: 0,
                    font_size_emu: LengthEmu::new(1000),
                    shaper_revision: "test".into(),
                },
                units_per_em: 1000,
                glyphs: vec![
                    BoundedShapedGlyph {
                        glyph_id: 10,
                        cluster: 2,
                        x_advance: LengthEmu::new(500),
                        y_advance: LengthEmu::ZERO,
                        x_offset: LengthEmu::ZERO,
                        y_offset: LengthEmu::ZERO,
                        unsafe_to_break: false,
                    },
                    BoundedShapedGlyph {
                        glyph_id: 11,
                        cluster: 3,
                        x_advance: LengthEmu::new(500),
                        y_advance: LengthEmu::ZERO,
                        x_offset: LengthEmu::ZERO,
                        y_offset: LengthEmu::ZERO,
                        unsafe_to_break: false,
                    },
                ],
                total_x_advance: LengthEmu::new(1000),
            },
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(1000),
            fill_rgb: [0, 0, 0],
        };

        let (glyphs, mappings) = prepare_run_glyphs(&run)
            .expect("Story-global clusters should map through scalar_base")
            .expect("bounded mapping should remain representable");
        assert_eq!(glyphs.len(), 2);
        assert_eq!(mappings.get(&10).map(String::as_str), Some("B"));
        assert_eq!(mappings.get(&11).map(String::as_str), Some("C"));
        assert_eq!(run.shaped.glyphs[0].cluster, 2);
        assert_eq!(run.shaped.glyphs[1].cluster, 3);

        let mut underflow = run.clone();
        underflow.scalar_base = 3;
        assert!(matches!(
            prepare_run_glyphs(&underflow),
            Err(PdfTextPreparationError::ClusterBeforeScalarBase {
                cluster: 2,
                scalar_base: 3,
                ..
            })
        ));
    }

    #[test]
    fn utf16_mapping_handles_non_bmp_scalars() {
        assert_eq!(utf16be_hex("A"), "0041");
        assert_eq!(utf16be_hex("😀"), "D83DDE00");
    }
}
