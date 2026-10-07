use anyhow::{Context, Result};
use pub_contents::MatureStoryCatalog;
use pub_core::StreamPath;
use pub_quill::{
    QuillFdppStoryCatalog, QuillGroundedStoryIdentity, QuillMcldChunk, QuillMcldReadError,
    QuillStoryCatalog, QuillStoryReadError, QuillTypographyCatalog,
    parse_bounded_fdpp_exact_story_catalog, parse_bounded_mcld, parse_bounded_typography,
    parse_confirmed_story_catalog,
};

use super::{PubBridgeDiagnostic, QUILL_STREAM_PATH};

pub(super) struct QuillAdmission {
    pub(super) quill_catalog: Option<QuillStoryCatalog>,
    pub(super) fdpp_story_catalog: Option<QuillFdppStoryCatalog>,
    pub(super) typography_catalog: Option<QuillTypographyCatalog>,
    pub(super) mcld: Option<QuillMcldChunk>,
}

pub(super) fn admit_quill_projection_inputs(
    quill: &[u8],
    grounded_story_catalog: Option<&MatureStoryCatalog>,
    physical_empty_story_catalog: bool,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<QuillAdmission> {
    let quill_stream = StreamPath(QUILL_STREAM_PATH.into());
    let mut fdpp_story_catalog = None;
    let quill_catalog = match parse_confirmed_story_catalog(quill_stream.clone(), quill) {
        Ok(catalog) => Some(catalog),
        Err(QuillStoryReadError::MissingRequiredChunk { name })
            if physical_empty_story_catalog && name == *b"STRS" =>
        {
            diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                reason: "physical-empty Story catalog has no live Story references and Quill omits STRS; admitting geometry-only publication without fabricating Story text".to_owned(),
            });
            None
        }
        Err(ordinary_error) => {
            let Some(story_catalog) = grounded_story_catalog else {
                return Err(ordinary_error).context("parse grounded Quill story catalog");
            };
            let identities = story_catalog
                .entries
                .iter()
                .map(|entry| QuillGroundedStoryIdentity {
                    syid: pub_core::QuillSyid(entry.text_id),
                    source: entry.text_id_source.clone(),
                })
                .collect::<Vec<_>>();
            match parse_bounded_fdpp_exact_story_catalog(quill_stream.clone(), quill, &identities)
                .context("parse bounded exact-FDPP Story fallback")?
            {
                Some(catalog) => {
                    diagnostics.push(PubBridgeDiagnostic::FdppExactStoryFallback {
                        story_count: catalog.stories.len(),
                    });
                    fdpp_story_catalog = Some(catalog);
                    None
                }
                None => {
                    return Err(ordinary_error).context("parse grounded Quill story catalog");
                }
            }
        }
    };

    let typography_catalog = if let Some(quill_catalog) = quill_catalog.as_ref() {
        match parse_bounded_typography(quill, quill_catalog) {
            Ok(catalog) => {
                let mut unknown = catalog.unknown_block_types_assumed_zero_length.clone();
                unknown.extend(
                    catalog
                        .inheritance_unknown_block_types_assumed_zero_length
                        .iter()
                        .copied(),
                );
                unknown.sort_unstable();
                unknown.dedup();
                if !unknown.is_empty() {
                    diagnostics.push(PubBridgeDiagnostic::TypographyUnknownFixedBlockTypes {
                        block_types: unknown,
                    });
                }
                Some(catalog)
            }
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::TypographyProjectionUnavailable {
                    reason: error.to_string(),
                });
                None
            }
        }
    } else {
        None
    };

    let mcld = if let Some(quill_catalog) = quill_catalog.as_ref() {
        match parse_bounded_mcld(quill_stream, quill, &quill_catalog.descriptor_nodes) {
            Ok(mcld) => Some(mcld),
            Err(QuillMcldReadError::MissingMcldDescriptor) => None,
            Err(QuillMcldReadError::RecordCountMismatch {
                record_count,
                record_id_count,
            }) => {
                diagnostics.push(PubBridgeDiagnostic::McldRecordCountMismatch {
                    record_count,
                    record_id_count,
                });
                None
            }
            Err(QuillMcldReadError::RecordIdOutsideOuterBound {
                outer_value,
                max_live_record_id,
            }) => {
                diagnostics.push(PubBridgeDiagnostic::McldOuterBoundViolation {
                    outer_value,
                    max_live_record_id,
                });
                None
            }
            Err(error) => return Err(error).context("parse bounded Quill MCLD"),
        }
    } else {
        None
    };

    Ok(QuillAdmission {
        quill_catalog,
        fdpp_story_catalog,
        typography_catalog,
        mcld,
    })
}
