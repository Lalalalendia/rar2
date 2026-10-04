use crate::seeded::inspect_pub_seed_manifest;
use pub_model::Sha256Digest;
use pub_reader::{
    CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH, QUILL_STREAM_PATH, build_mature_0x2c_source_graph,
    resolve_pub_source_graph,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;

pub const PUB_BOOTSTRAP_NEW_DOC_SCHEMA_V0_1: &str = "chaptera.pub-bootstrap-new-doc/v0.1";
pub const PUB_BOOTSTRAP_NEW_DOC_TASK: &str = "PUB-BOOTSTRAP-NEW-DOC-01";
pub const PUB_BOOTSTRAP_RECTANGLE_SEQ_NUM: u32 = 293;
pub const PUB_BOOTSTRAP_TEXTBOX_SEQ_NUM: u32 = 294;

const BOOTSTRAP_SEED_HASH: [u8; 32] = [
    0xd3, 0xc0, 0xa4, 0x85, 0x34, 0xe5, 0x77, 0x61, 0x05, 0x6a, 0xeb, 0x0e, 0xf9, 0x46, 0xf2, 0x55,
    0x8d, 0xaf, 0xae, 0x5b, 0x4f, 0x58, 0x86, 0xc3, 0x2c, 0xe8, 0x83, 0xff, 0x49, 0x57, 0xba, 0x13,
];
const BOOTSTRAP_CONTENTS_HASH: [u8; 32] = [
    0x86, 0x8e, 0xdc, 0x28, 0xe7, 0x61, 0x55, 0xbe, 0xd5, 0xc2, 0xd4, 0x93, 0x37, 0x05, 0xae, 0x4d,
    0x2d, 0x8d, 0x25, 0xf4, 0x9f, 0x7e, 0x5e, 0x82, 0xb0, 0x35, 0xae, 0xda, 0x8f, 0x3e, 0x23, 0x85,
];
const BOOTSTRAP_ESCHER_HASH: [u8; 32] = [
    0xff, 0x86, 0x7d, 0x61, 0x19, 0x1d, 0x78, 0xf3, 0x06, 0x0d, 0x0b, 0x08, 0xf5, 0xe6, 0x08, 0x0e,
    0x5f, 0x13, 0x79, 0xad, 0x7a, 0xee, 0xb5, 0xb0, 0x00, 0xa6, 0xaf, 0x14, 0xad, 0x2a, 0xa7, 0x95,
];
const BOOTSTRAP_QUILL_HASH: [u8; 32] = [
    0x04, 0xaf, 0x6e, 0x13, 0x38, 0x6f, 0x40, 0x93, 0x73, 0x9d, 0xd0, 0xb2, 0x4e, 0xf7, 0x39, 0x50,
    0xdc, 0xb8, 0x23, 0x75, 0x03, 0xa9, 0x8e, 0xd3, 0xf1, 0xdd, 0xa7, 0xdf, 0x5b, 0x50, 0x05, 0xa6,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapNewDocTemplate<'a> {
    pub contents_stream: &'a [u8],
    pub escher_stream: &'a [u8],
    pub quill_stream: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapNewDocStreamDelta {
    pub path: String,
    pub before_len: u64,
    pub after_len: u64,
    pub before_sha256: Sha256Digest,
    pub after_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapNewDocReport {
    pub schema: &'static str,
    pub task: &'static str,
    pub seed_sha256: Sha256Digest,
    pub output_sha256: Sha256Digest,
    pub changed_streams: Vec<BootstrapNewDocStreamDelta>,
    pub preserved_stream_count: usize,
    pub page_count: usize,
    pub node_count: usize,
    pub story_count: usize,
    pub rectangle_contents_seq_num: u32,
    pub textbox_contents_seq_num: u32,
    pub textbox_story_text: String,
    pub scope: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapNewDocCandidate {
    pub report: BootstrapNewDocReport,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapNewDocBlocked {
    SeedHashMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    TemplateHashMismatch {
        path: &'static str,
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    SeedManifest {
        detail: String,
    },
    CfbMaterialization {
        path: &'static str,
        detail: String,
    },
    OutputManifest {
        detail: String,
    },
    TopologyChanged,
    UnexpectedStreamMutation {
        path: String,
    },
    MissingOwnedStreamMutation {
        path: &'static str,
    },
    ReopenSourceGraph {
        detail: String,
    },
    ReopenResolve {
        detail: String,
    },
    OutputSemantic {
        detail: String,
    },
}

impl BootstrapNewDocBlocked {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SeedHashMismatch { .. } => "seed_hash_mismatch",
            Self::TemplateHashMismatch { .. } => "template_hash_mismatch",
            Self::SeedManifest { .. } => "seed_manifest",
            Self::CfbMaterialization { .. } => "cfb_materialization",
            Self::OutputManifest { .. } => "output_manifest",
            Self::TopologyChanged => "cfb_topology_changed",
            Self::UnexpectedStreamMutation { .. } => "unexpected_stream_mutation",
            Self::MissingOwnedStreamMutation { .. } => "missing_owned_stream_mutation",
            Self::ReopenSourceGraph { .. } => "reopen_source_graph",
            Self::ReopenResolve { .. } => "reopen_resolve",
            Self::OutputSemantic { .. } => "output_semantic",
        }
    }
}

impl fmt::Display for BootstrapNewDocBlocked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeedHashMismatch { expected, actual } => {
                write!(
                    formatter,
                    "bootstrap seed SHA mismatch: expected {expected}, got {actual}"
                )
            }
            Self::TemplateHashMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "bootstrap template hash mismatch for {path}: expected {expected}, got {actual}"
            ),
            Self::SeedManifest { detail } => {
                write!(formatter, "bootstrap seed manifest failed: {detail}")
            }
            Self::CfbMaterialization { path, detail } => {
                write!(
                    formatter,
                    "bootstrap CFB replacement failed for {path}: {detail}"
                )
            }
            Self::OutputManifest { detail } => {
                write!(formatter, "bootstrap output manifest failed: {detail}")
            }
            Self::TopologyChanged => formatter.write_str("bootstrap changed CFB logical topology"),
            Self::UnexpectedStreamMutation { path } => {
                write!(formatter, "bootstrap changed unowned logical stream {path}")
            }
            Self::MissingOwnedStreamMutation { path } => {
                write!(
                    formatter,
                    "bootstrap did not change required owned stream {path}"
                )
            }
            Self::ReopenSourceGraph { detail } => {
                write!(
                    formatter,
                    "bootstrap output SourceGraph reopen failed: {detail}"
                )
            }
            Self::ReopenResolve { detail } => {
                write!(
                    formatter,
                    "bootstrap output semantic resolve failed: {detail}"
                )
            }
            Self::OutputSemantic { detail } => {
                write!(
                    formatter,
                    "bootstrap output semantic check failed: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for BootstrapNewDocBlocked {}

pub fn materialize_bounded_bootstrap_new_doc_candidate(
    seed_pub: &[u8],
    template: BootstrapNewDocTemplate<'_>,
) -> Result<BootstrapNewDocCandidate, BootstrapNewDocBlocked> {
    let seed_hash = sha256_digest(seed_pub);
    let expected_seed = Sha256Digest::from_bytes(BOOTSTRAP_SEED_HASH);
    if seed_hash != expected_seed {
        return Err(BootstrapNewDocBlocked::SeedHashMismatch {
            expected: expected_seed,
            actual: seed_hash,
        });
    }

    require_template_hash(
        CONTENTS_STREAM_PATH,
        template.contents_stream,
        BOOTSTRAP_CONTENTS_HASH,
    )?;
    require_template_hash(
        ESCHER_STREAM_PATH,
        template.escher_stream,
        BOOTSTRAP_ESCHER_HASH,
    )?;
    require_template_hash(
        QUILL_STREAM_PATH,
        template.quill_stream,
        BOOTSTRAP_QUILL_HASH,
    )?;

    let seed_manifest = inspect_pub_seed_manifest(seed_pub).map_err(|error| {
        BootstrapNewDocBlocked::SeedManifest {
            detail: error.to_string(),
        }
    })?;

    let with_contents = replace_stream(seed_pub, CONTENTS_STREAM_PATH, template.contents_stream)?;
    let with_escher = replace_stream(&with_contents, ESCHER_STREAM_PATH, template.escher_stream)?;
    let output = replace_stream(&with_escher, QUILL_STREAM_PATH, template.quill_stream)?;

    let output_hash = sha256_digest(&output);
    let output_manifest = inspect_pub_seed_manifest(&output).map_err(|error| {
        BootstrapNewDocBlocked::OutputManifest {
            detail: error.to_string(),
        }
    })?;
    let (changed_streams, preserved_stream_count) =
        validate_preservation(&seed_manifest, &output_manifest)?;

    let reopened =
        build_mature_0x2c_source_graph(Cursor::new(&output), output_hash).map_err(|error| {
            BootstrapNewDocBlocked::ReopenSourceGraph {
                detail: format!("{error:#}"),
            }
        })?;
    let resolved = resolve_pub_source_graph(&reopened.graph).map_err(|error| {
        BootstrapNewDocBlocked::ReopenResolve {
            detail: format!("{error:#}"),
        }
    })?;

    if resolved.graph.document.pages.len() != 1 || resolved.graph.pages.len() != 1 {
        return Err(BootstrapNewDocBlocked::OutputSemantic {
            detail: format!(
                "expected one page, document has {} ordered pages and {} page entities",
                resolved.graph.document.pages.len(),
                resolved.graph.pages.len()
            ),
        });
    }

    let rectangle = resolved
        .graph
        .nodes
        .values()
        .find(|node| node.payload.contents_seq_num == PUB_BOOTSTRAP_RECTANGLE_SEQ_NUM)
        .ok_or_else(|| BootstrapNewDocBlocked::OutputSemantic {
            detail: format!(
                "missing rectangle Contents seq {}",
                PUB_BOOTSTRAP_RECTANGLE_SEQ_NUM
            ),
        })?;
    let textbox = resolved
        .graph
        .nodes
        .values()
        .find(|node| node.payload.contents_seq_num == PUB_BOOTSTRAP_TEXTBOX_SEQ_NUM)
        .ok_or_else(|| BootstrapNewDocBlocked::OutputSemantic {
            detail: format!(
                "missing textbox Contents seq {}",
                PUB_BOOTSTRAP_TEXTBOX_SEQ_NUM
            ),
        })?;

    let page_id = resolved.graph.document.pages[0];
    let page = resolved.graph.pages.get(&page_id).ok_or_else(|| {
        BootstrapNewDocBlocked::OutputSemantic {
            detail: "ordered customer page is missing from page registry".into(),
        }
    })?;
    for node in [rectangle, textbox] {
        if !page.children.contains(&node.header.id) {
            return Err(BootstrapNewDocBlocked::OutputSemantic {
                detail: format!(
                    "page does not own expected Contents seq {} node",
                    node.payload.contents_seq_num
                ),
            });
        }
    }

    let story_id = textbox
        .payload
        .story_frame
        .as_ref()
        .and_then(|frame| frame.story_id)
        .ok_or_else(|| BootstrapNewDocBlocked::OutputSemantic {
            detail: "textbox seq294 has no resolved Story identity".into(),
        })?;
    let story = resolved.graph.stories.get(&story_id).ok_or_else(|| {
        BootstrapNewDocBlocked::OutputSemantic {
            detail: "textbox Story identity is absent from Story registry".into(),
        }
    })?;
    if story.text.trim_end_matches('\r') != "Hello" {
        return Err(BootstrapNewDocBlocked::OutputSemantic {
            detail: format!("textbox Story text mismatch: {:?}", story.text),
        });
    }

    Ok(BootstrapNewDocCandidate {
        report: BootstrapNewDocReport {
            schema: PUB_BOOTSTRAP_NEW_DOC_SCHEMA_V0_1,
            task: PUB_BOOTSTRAP_NEW_DOC_TASK,
            seed_sha256: seed_hash,
            output_sha256: output_hash,
            changed_streams,
            preserved_stream_count,
            page_count: resolved.graph.pages.len(),
            node_count: resolved.graph.nodes.len(),
            story_count: resolved.graph.stories.len(),
            rectangle_contents_seq_num: rectangle.payload.contents_seq_num,
            textbox_contents_seq_num: textbox.payload.contents_seq_num,
            textbox_story_text: story.text.clone(),
            scope: "Publisher2019/build12527 exact T537 blank seed; one captured rectangle + one captured textbox Hello composite; Contents path provenance scrubbed at equal UTF-16 length; not a generic allocator or zero-seed writer",
        },
        bytes: output,
    })
}

pub fn bootstrap_new_doc_report_json(
    report: &BootstrapNewDocReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

fn replace_stream(
    source: &[u8],
    path: &'static str,
    stream: &[u8],
) -> Result<Vec<u8>, BootstrapNewDocBlocked> {
    pub_cfb::replace_stream_reader(Cursor::new(source), path, stream).map_err(|error| {
        BootstrapNewDocBlocked::CfbMaterialization {
            path,
            detail: format!("{error:#}"),
        }
    })
}

fn require_template_hash(
    path: &'static str,
    bytes: &[u8],
    expected: [u8; 32],
) -> Result<(), BootstrapNewDocBlocked> {
    let actual = sha256_digest(bytes);
    let expected = Sha256Digest::from_bytes(expected);
    if actual != expected {
        return Err(BootstrapNewDocBlocked::TemplateHashMismatch {
            path,
            expected,
            actual,
        });
    }
    Ok(())
}

fn validate_preservation(
    seed: &crate::PubSeedManifest,
    output: &crate::PubSeedManifest,
) -> Result<(Vec<BootstrapNewDocStreamDelta>, usize), BootstrapNewDocBlocked> {
    let seed_topology = seed
        .inventory
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.kind))
        .collect::<Vec<_>>();
    let output_topology = output
        .inventory
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.kind))
        .collect::<Vec<_>>();
    if seed_topology != output_topology {
        return Err(BootstrapNewDocBlocked::TopologyChanged);
    }

    let owned = [CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH, QUILL_STREAM_PATH];
    let seed_streams = seed
        .streams
        .iter()
        .map(|stream| (stream.path.as_str(), stream))
        .collect::<BTreeMap<_, _>>();
    let output_streams = output
        .streams
        .iter()
        .map(|stream| (stream.path.as_str(), stream))
        .collect::<BTreeMap<_, _>>();

    let mut changed = Vec::new();
    let mut preserved = 0usize;
    for (path, before) in &seed_streams {
        let after =
            output_streams
                .get(path)
                .ok_or_else(|| BootstrapNewDocBlocked::OutputManifest {
                    detail: format!("output lost logical stream {path}"),
                })?;
        if before.len == after.len && before.sha256 == after.sha256 {
            preserved += 1;
            continue;
        }
        if !owned.contains(path) {
            return Err(BootstrapNewDocBlocked::UnexpectedStreamMutation {
                path: (*path).to_owned(),
            });
        }
        changed.push(BootstrapNewDocStreamDelta {
            path: (*path).to_owned(),
            before_len: before.len,
            after_len: after.len,
            before_sha256: before.sha256,
            after_sha256: after.sha256,
        });
    }

    for path in owned {
        if !changed.iter().any(|delta| delta.path == path) {
            return Err(BootstrapNewDocBlocked::MissingOwnedStreamMutation { path });
        }
    }

    Ok((changed, preserved))
}

fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&digest);
    Sha256Digest::from_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_seed_is_rejected_before_cfb_parsing() {
        let error = materialize_bounded_bootstrap_new_doc_candidate(
            b"not the pinned blank",
            BootstrapNewDocTemplate {
                contents_stream: b"wrong",
                escher_stream: b"wrong",
                quill_stream: b"wrong",
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "seed_hash_mismatch");
    }

    #[test]
    fn task_and_owned_projection_contract_are_stable() {
        assert_eq!(PUB_BOOTSTRAP_NEW_DOC_TASK, "PUB-BOOTSTRAP-NEW-DOC-01");
        assert_eq!(PUB_BOOTSTRAP_RECTANGLE_SEQ_NUM, 293);
        assert_eq!(PUB_BOOTSTRAP_TEXTBOX_SEQ_NUM, 294);
        assert_eq!(CONTENTS_STREAM_PATH, "/Contents");
        assert_eq!(ESCHER_STREAM_PATH, "/Escher/EscherStm");
        assert_eq!(QUILL_STREAM_PATH, "/Quill/QuillSub/CONTENTS");
    }
}
