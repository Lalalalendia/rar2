use pub_contents::{
    RawContentsBlockBody, parse_0x2c_header, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, inspect_sp_containers};
use pub_model::{LengthEmu, Sha256Digest};
use pub_reader::{
    CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH, build_mature_0x2c_structural_base_manifest,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;

pub const STRUCT_WRITER_REPLAY_SCHEMA_V0_1: &str = "pub-struct-writer-replay-plan/v0.1";

pub const T406_SHAPE_SEQ_NUM: u32 = 305;
pub const T406_RELOCATED_SERVICE_SEQ_NUM: u32 = 306;
pub const T406_PAGE_PARENT_SEQ_NUM: u32 = 266;
pub const T406_SERVICE_PARENT_SEQ_NUM: u32 = 282;
pub const T406_NEW_SPID: u32 = 1032;

const T406_CAPTURE_SOURCE_HASH: [u8; 32] = [
    0xa1, 0x63, 0x7f, 0x89, 0x1e, 0x25, 0xfe, 0x10, 0x94, 0x5a, 0x30, 0xf2, 0x70, 0x00, 0xf8, 0xa9,
    0xab, 0xe6, 0x6f, 0xdd, 0xf3, 0xc9, 0x49, 0xd1, 0xad, 0xe8, 0x84, 0x33, 0xe1, 0xc8, 0x36, 0xd3,
];
const T406_NATIVE_CONTENTS_HASH: [u8; 32] = [
    0x4c, 0xf0, 0x77, 0xd9, 0x1e, 0x7b, 0xed, 0x73, 0x0c, 0x17, 0x41, 0x14, 0x63, 0xf0, 0xb3, 0x18,
    0x71, 0x1b, 0xd8, 0x8b, 0x29, 0xa9, 0x37, 0xd8, 0xb3, 0x1a, 0x11, 0xb2, 0x69, 0xee, 0xe4, 0x68,
];
const T406_NATIVE_ESCHER_HASH: [u8; 32] = [
    0x4f, 0xa7, 0xb4, 0x80, 0x84, 0xa5, 0x17, 0xa3, 0x8b, 0xe5, 0xd2, 0x63, 0xae, 0x5a, 0x73, 0xd6,
    0xb6, 0x72, 0x60, 0x57, 0xfb, 0x32, 0x60, 0x2c, 0xd2, 0xf8, 0x32, 0x62, 0xcc, 0x58, 0xc9, 0x24,
];

const T406_AUTHORED_LEFT_EMU: i64 = 1_003_300;
const T406_AUTHORED_TOP_EMU: i64 = 1_435_100;
const T406_AUTHORED_WIDTH_EMU: i64 = 2_298_700;
const T406_AUTHORED_HEIGHT_EMU: i64 = 927_100;
const T406_AUTHORED_FILL_RGB: u32 = 0x0000_A5FF;
const T406_AUTHORED_LINE_WIDTH_EMU: u32 = 28_575;

const T406_PERSISTED_X_EMU: i64 = 1_003_316;
const T406_PERSISTED_Y_EMU: i64 = 1_435_096;
const T406_PERSISTED_WIDTH_EMU: i64 = 2_298_688;
const T406_PERSISTED_HEIGHT_EMU: i64 = 927_104;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OrdinaryShapeReplayGeometry {
    pub left_emu: i64,
    pub top_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryShapeReplayRequest {
    pub expected_source_hash: Sha256Digest,
    pub geometry: OrdinaryShapeReplayGeometry,
    pub fill_rgb: u32,
    pub line_width_emu: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructuralReplayObservedBase {
    pub source_hash: Sha256Digest,
    pub stream_count: usize,
    pub candidate_count: usize,
    pub slot_count: u32,
    pub max_ordinal: u32,
    pub t370_contents_seq_num: u32,
    pub t370_parent_seq_num: u32,
    pub t370_spid: u32,
    pub t370_shape_type: u16,
    pub t370_width_emu: i64,
    pub t370_height_emu: i64,
    pub service_seq_num: u32,
    pub service_raw_type: u16,
    pub service_parent_seq_num: u32,
    pub service_field_06: u16,
    pub service_field_0b: u16,
    pub max_observed_spid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrdinaryShapeReplayPlan {
    pub schema: &'static str,
    pub source_hash: Sha256Digest,
    pub source_slot_count: u32,
    pub source_max_ordinal: u32,
    pub new_shape_seq_num: u32,
    pub relocated_service_seq_num: u32,
    pub page_parent_seq_num: u32,
    pub service_parent_seq_num: u32,
    pub new_spid: u32,
    pub geometry: OrdinaryShapeReplayGeometry,
    pub fill_rgb: u32,
    pub line_width_emu: u32,
    pub new_shape_directory_field_06: u16,
    pub new_shape_directory_field_0b: u16,
    pub relocated_service_field_06: u16,
    pub relocated_service_field_0b: u16,
    pub oid_mutation_required: bool,
    pub dw_next_unique_oid_mutation_required: bool,
    pub owned_streams: [&'static str; 2],
    pub allocator_scope: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrdinaryShapeReplayTemplate<'a> {
    pub contents_stream: &'a [u8],
    pub escher_stream: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructuralReplayStreamDelta {
    pub path: String,
    pub before_len: u64,
    pub after_len: u64,
    pub before_sha256: Sha256Digest,
    pub after_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralReplayPubCandidate {
    pub plan: OrdinaryShapeReplayPlan,
    pub source_hash: Sha256Digest,
    pub output_hash: Sha256Digest,
    pub changed_streams: Vec<StructuralReplayStreamDelta>,
    pub preserved_stream_count: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralReplayDeleteCandidate {
    pub source_hash: Sha256Digest,
    pub created_hash: Sha256Digest,
    pub output_hash: Sha256Digest,
    pub changed_streams: Vec<StructuralReplayStreamDelta>,
    pub preserved_stream_count: usize,
    pub all_logical_streams_equal_source: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralReplayMaterializationBlocked {
    Plan(StructuralReplayPlanBlocked),
    CaptureSourceHashMismatch {
        actual: Sha256Digest,
    },
    RequestMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    TemplateHashMismatch {
        path: &'static str,
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    Cfb {
        detail: String,
    },
    UnexpectedStreamMutation {
        path: String,
    },
    MissingOwnedStreamMutation {
        path: &'static str,
    },
    OutputValidation {
        detail: String,
    },
}

impl StructuralReplayMaterializationBlocked {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Plan(error) => error.code(),
            Self::CaptureSourceHashMismatch { .. } => "capture_source_hash_mismatch",
            Self::RequestMismatch { .. } => "request_mismatch",
            Self::TemplateHashMismatch { .. } => "template_hash_mismatch",
            Self::Cfb { .. } => "cfb_materialization",
            Self::UnexpectedStreamMutation { .. } => "unexpected_stream_mutation",
            Self::MissingOwnedStreamMutation { .. } => "missing_owned_stream_mutation",
            Self::OutputValidation { .. } => "output_validation",
        }
    }
}

impl fmt::Display for StructuralReplayMaterializationBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "T406 replay plan rejected: {error}"),
            Self::CaptureSourceHashMismatch { actual } => write!(
                f,
                "T406 capture-bound source SHA mismatch: expected {}, got {actual}",
                Sha256Digest::from_bytes(T406_CAPTURE_SOURCE_HASH)
            ),
            Self::RequestMismatch {
                field,
                expected,
                actual,
            } => write!(
                f,
                "T406 captured request {field} mismatch: expected {expected}, got {actual}"
            ),
            Self::TemplateHashMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "T406 captured template hash mismatch for {path}: expected {expected}, got {actual}"
            ),
            Self::Cfb { detail } => write!(f, "T406 CFB materialization failed: {detail}"),
            Self::UnexpectedStreamMutation { path } => {
                write!(f, "T406 replay changed unowned logical stream {path}")
            }
            Self::MissingOwnedStreamMutation { path } => {
                write!(f, "T406 replay did not change required owned stream {path}")
            }
            Self::OutputValidation { detail } => {
                write!(f, "T406 output validation failed: {detail}")
            }
        }
    }
}

impl std::error::Error for StructuralReplayMaterializationBlocked {}

impl From<StructuralReplayPlanBlocked> for StructuralReplayMaterializationBlocked {
    fn from(value: StructuralReplayPlanBlocked) -> Self {
        Self::Plan(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralReplayPlanBlocked {
    SourceHashMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    CfbRead {
        path: &'static str,
        detail: String,
    },
    Contents {
        detail: String,
    },
    Escher {
        detail: String,
    },
    StructuralBase {
        detail: String,
    },
    Precondition {
        field: &'static str,
        expected: String,
        actual: String,
    },
}

impl StructuralReplayPlanBlocked {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SourceHashMismatch { .. } => "source_hash_mismatch",
            Self::CfbRead { .. } => "cfb_read",
            Self::Contents { .. } => "contents_parse",
            Self::Escher { .. } => "escher_parse",
            Self::StructuralBase { .. } => "structural_base",
            Self::Precondition { .. } => "precondition_mismatch",
        }
    }
}

impl fmt::Display for StructuralReplayPlanBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceHashMismatch { expected, actual } => {
                write!(f, "source hash mismatch: expected {expected}, got {actual}")
            }
            Self::CfbRead { path, detail } => write!(f, "cannot read {path}: {detail}"),
            Self::Contents { detail } => write!(f, "Contents parse failed: {detail}"),
            Self::Escher { detail } => write!(f, "Escher parse failed: {detail}"),
            Self::StructuralBase { detail } => write!(f, "structural-base parse failed: {detail}"),
            Self::Precondition {
                field,
                expected,
                actual,
            } => write!(
                f,
                "T406 bounded precondition {field} mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for StructuralReplayPlanBlocked {}

pub fn inspect_t370_ordinary_shape_replay_base(
    source_pub: &[u8],
) -> Result<StructuralReplayObservedBase, StructuralReplayPlanBlocked> {
    let source_hash = sha256_digest(source_pub);

    let contents = pub_cfb::read_stream_reader(Cursor::new(source_pub), CONTENTS_STREAM_PATH)
        .map_err(|error| StructuralReplayPlanBlocked::CfbRead {
            path: CONTENTS_STREAM_PATH,
            detail: format!("{error:#}"),
        })?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents).map_err(|error| {
        StructuralReplayPlanBlocked::Contents {
            detail: error.to_string(),
        }
    })?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header).map_err(|error| {
        StructuralReplayPlanBlocked::Contents {
            detail: error.to_string(),
        }
    })?;

    let shape_reference =
        parse_confirmed_chunk_reference(&contents, &trailer.directory, 293).map_err(|error| {
            StructuralReplayPlanBlocked::Contents {
                detail: error.to_string(),
            }
        })?;
    let shape_reference = shape_reference.ok_or_else(|| StructuralReplayPlanBlocked::Precondition {
        field: "seq293",
        expected: "occupied shape reference".into(),
        actual: "empty".into(),
    })?;

    let service_reference =
        parse_confirmed_chunk_reference(&contents, &trailer.directory, 305).map_err(|error| {
            StructuralReplayPlanBlocked::Contents {
                detail: error.to_string(),
            }
        })?;
    let service_reference = service_reference.ok_or_else(|| {
        StructuralReplayPlanBlocked::Precondition {
            field: "seq305",
            expected: "occupied service reference".into(),
            actual: "empty".into(),
        }
    })?;

    let manifest = build_mature_0x2c_structural_base_manifest(source_pub).map_err(|error| {
        StructuralReplayPlanBlocked::StructuralBase {
            detail: format!("{error:#}"),
        }
    })?;
    let t370_matches = manifest
        .candidates
        .iter()
        .filter(|candidate| candidate.contents_seq_num == 293)
        .collect::<Vec<_>>();
    let [t370] = t370_matches.as_slice() else {
        return Err(StructuralReplayPlanBlocked::Precondition {
            field: "T370 candidate seq293",
            expected: "exactly one candidate".into(),
            actual: t370_matches.len().to_string(),
        });
    };

    let width = unique_u32_field(&t370.contents_chunk.fields, 0x00AA)
        .map(i64::from)
        .ok_or_else(|| StructuralReplayPlanBlocked::Precondition {
            field: "T370 width 0xAA",
            expected: "single u32".into(),
            actual: "missing/ambiguous".into(),
        })?;
    let height = unique_u32_field(&t370.contents_chunk.fields, 0x00AB)
        .map(i64::from)
        .ok_or_else(|| StructuralReplayPlanBlocked::Precondition {
            field: "T370 height 0xAB",
            expected: "single u32".into(),
            actual: "missing/ambiguous".into(),
        })?;

    let escher = pub_cfb::read_stream_reader(Cursor::new(source_pub), ESCHER_STREAM_PATH)
        .map_err(|error| StructuralReplayPlanBlocked::CfbRead {
            path: ESCHER_STREAM_PATH,
            detail: format!("{error:#}"),
        })?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), &escher)
        .map_err(|error| StructuralReplayPlanBlocked::Escher {
            detail: error.to_string(),
        })?;
    let max_observed_spid = inventory
        .shapes
        .iter()
        .filter_map(|shape| shape.fsp.as_ref().map(|fsp| fsp.spid))
        .max()
        .unwrap_or(0);

    Ok(StructuralReplayObservedBase {
        source_hash,
        stream_count: manifest.streams.len(),
        candidate_count: manifest.candidates.len(),
        slot_count: trailer.slot_count,
        max_ordinal: trailer.max_ordinal,
        t370_contents_seq_num: 293,
        t370_parent_seq_num: unique_observed_u32(&shape_reference.parent_seq_nums)
            .unwrap_or(u32::MAX),
        t370_spid: t370.officeart_spid,
        t370_shape_type: t370.officeart_shape_type,
        t370_width_emu: width,
        t370_height_emu: height,
        service_seq_num: 305,
        service_raw_type: unique_observed_u16(&service_reference.raw_types).unwrap_or(u16::MAX),
        service_parent_seq_num: unique_observed_u32(&service_reference.parent_seq_nums)
            .unwrap_or(u32::MAX),
        service_field_06: unique_u16_field(&service_reference.fields, 0x06).unwrap_or(u16::MAX),
        service_field_0b: unique_u16_field(&service_reference.fields, 0x0B).unwrap_or(u16::MAX),
        max_observed_spid,
    })
}

pub fn plan_bounded_t406_ordinary_shape_replay(
    observed: &StructuralReplayObservedBase,
    request: &OrdinaryShapeReplayRequest,
) -> Result<OrdinaryShapeReplayPlan, StructuralReplayPlanBlocked> {
    if observed.source_hash != request.expected_source_hash {
        return Err(StructuralReplayPlanBlocked::SourceHashMismatch {
            expected: request.expected_source_hash,
            actual: observed.source_hash,
        });
    }

    require("stream_count", observed.stream_count, 10)?;
    require("candidate_count", observed.candidate_count, 6)?;
    require("slot_count", observed.slot_count, 306)?;
    require("max_ordinal", observed.max_ordinal, 305)?;
    require("t370_contents_seq_num", observed.t370_contents_seq_num, 293)?;
    require(
        "t370_parent_seq_num",
        observed.t370_parent_seq_num,
        T406_PAGE_PARENT_SEQ_NUM,
    )?;
    require("t370_spid", observed.t370_spid, 1025)?;
    require("t370_shape_type", observed.t370_shape_type, 202)?;
    require("t370_width_emu", observed.t370_width_emu, 5_076_000)?;
    require("t370_height_emu", observed.t370_height_emu, 972_000)?;
    require(
        "service_seq_num",
        observed.service_seq_num,
        T406_SHAPE_SEQ_NUM,
    )?;
    require("service_raw_type", observed.service_raw_type, 0x006C)?;
    require(
        "service_parent_seq_num",
        observed.service_parent_seq_num,
        T406_SERVICE_PARENT_SEQ_NUM,
    )?;
    require("service_field_06", observed.service_field_06, 9474)?;
    require("service_field_0b", observed.service_field_0b, 1)?;
    require("max_observed_spid", observed.max_observed_spid, 1031)?;

    if request.geometry.width_emu <= 0 || request.geometry.height_emu <= 0 {
        return Err(StructuralReplayPlanBlocked::Precondition {
            field: "requested geometry",
            expected: "positive width and height".into(),
            actual: format!(
                "{}x{}",
                request.geometry.width_emu, request.geometry.height_emu
            ),
        });
    }

    Ok(OrdinaryShapeReplayPlan {
        schema: STRUCT_WRITER_REPLAY_SCHEMA_V0_1,
        source_hash: observed.source_hash,
        source_slot_count: observed.slot_count,
        source_max_ordinal: observed.max_ordinal,
        new_shape_seq_num: T406_SHAPE_SEQ_NUM,
        relocated_service_seq_num: T406_RELOCATED_SERVICE_SEQ_NUM,
        page_parent_seq_num: T406_PAGE_PARENT_SEQ_NUM,
        service_parent_seq_num: T406_SERVICE_PARENT_SEQ_NUM,
        new_spid: T406_NEW_SPID,
        geometry: request.geometry,
        fill_rgb: request.fill_rgb,
        line_width_emu: request.line_width_emu,
        new_shape_directory_field_06: observed.service_field_06,
        new_shape_directory_field_0b: 22,
        relocated_service_field_06: observed.service_field_06,
        relocated_service_field_0b: observed.service_field_0b,
        oid_mutation_required: false,
        dw_next_unique_oid_mutation_required: false,
        owned_streams: [CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH],
        allocator_scope:
            "Publisher2019/build12527 T370-equivalent ordinary non-text creation only",
    })
}

pub fn plan_bounded_t406_ordinary_shape_replay_from_pub(
    source_pub: &[u8],
    request: &OrdinaryShapeReplayRequest,
) -> Result<OrdinaryShapeReplayPlan, StructuralReplayPlanBlocked> {
    let observed = inspect_t370_ordinary_shape_replay_base(source_pub)?;
    plan_bounded_t406_ordinary_shape_replay(&observed, request)
}

pub fn materialize_bounded_t406_create_pub_candidate(
    source_pub: &[u8],
    request: &OrdinaryShapeReplayRequest,
    template: OrdinaryShapeReplayTemplate<'_>,
) -> Result<StructuralReplayPubCandidate, StructuralReplayMaterializationBlocked> {
    let source_hash = sha256_digest(source_pub);
    if source_hash != Sha256Digest::from_bytes(T406_CAPTURE_SOURCE_HASH) {
        return Err(StructuralReplayMaterializationBlocked::CaptureSourceHashMismatch {
            actual: source_hash,
        });
    }
    require_captured_request(request)?;
    require_template_hash(
        CONTENTS_STREAM_PATH,
        template.contents_stream,
        T406_NATIVE_CONTENTS_HASH,
    )?;
    require_template_hash(
        ESCHER_STREAM_PATH,
        template.escher_stream,
        T406_NATIVE_ESCHER_HASH,
    )?;

    let plan = plan_bounded_t406_ordinary_shape_replay_from_pub(source_pub, request)?;

    let with_contents = pub_cfb::replace_stream_reader(
        Cursor::new(source_pub),
        CONTENTS_STREAM_PATH,
        template.contents_stream,
    )
    .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
        detail: format!("{error:#}"),
    })?;
    let output = pub_cfb::replace_stream_reader(
        Cursor::new(&with_contents),
        ESCHER_STREAM_PATH,
        template.escher_stream,
    )
    .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
        detail: format!("{error:#}"),
    })?;

    let (changed_streams, preserved_stream_count) =
        validate_owned_stream_changes(source_pub, &output)?;
    validate_created_candidate(&output)?;

    Ok(StructuralReplayPubCandidate {
        plan,
        source_hash,
        output_hash: sha256_digest(&output),
        changed_streams,
        preserved_stream_count,
        bytes: output,
    })
}

pub fn materialize_bounded_t406_delete_pub_candidate(
    source_pub: &[u8],
    created_pub: &[u8],
) -> Result<StructuralReplayDeleteCandidate, StructuralReplayMaterializationBlocked> {
    let source_hash = sha256_digest(source_pub);
    if source_hash != Sha256Digest::from_bytes(T406_CAPTURE_SOURCE_HASH) {
        return Err(StructuralReplayMaterializationBlocked::CaptureSourceHashMismatch {
            actual: source_hash,
        });
    }
    validate_created_candidate(created_pub)?;

    let created_contents = pub_cfb::read_stream_reader(Cursor::new(created_pub), CONTENTS_STREAM_PATH)
        .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        })?;
    require_template_hash(
        CONTENTS_STREAM_PATH,
        &created_contents,
        T406_NATIVE_CONTENTS_HASH,
    )?;
    let created_escher = pub_cfb::read_stream_reader(Cursor::new(created_pub), ESCHER_STREAM_PATH)
        .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        })?;
    require_template_hash(
        ESCHER_STREAM_PATH,
        &created_escher,
        T406_NATIVE_ESCHER_HASH,
    )?;

    let source_contents = pub_cfb::read_stream_reader(Cursor::new(source_pub), CONTENTS_STREAM_PATH)
        .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        })?;
    let source_escher = pub_cfb::read_stream_reader(Cursor::new(source_pub), ESCHER_STREAM_PATH)
        .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        })?;

    let with_contents = pub_cfb::replace_stream_reader(
        Cursor::new(created_pub),
        CONTENTS_STREAM_PATH,
        &source_contents,
    )
    .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
        detail: format!("{error:#}"),
    })?;
    let output = pub_cfb::replace_stream_reader(
        Cursor::new(&with_contents),
        ESCHER_STREAM_PATH,
        &source_escher,
    )
    .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
        detail: format!("{error:#}"),
    })?;

    let (changed_streams, preserved_stream_count) =
        validate_owned_stream_changes(created_pub, &output)?;
    let all_logical_streams_equal_source = logical_streams_equal(source_pub, &output)?;
    if !all_logical_streams_equal_source {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: "delete replay did not restore every logical stream to the source state".into(),
        });
    }

    let observed = inspect_t370_ordinary_shape_replay_base(&output)?;
    require("delete stream_count", observed.stream_count, 10)?;
    require("delete candidate_count", observed.candidate_count, 6)?;
    require("delete slot_count", observed.slot_count, 306)?;
    require("delete max_ordinal", observed.max_ordinal, 305)?;
    require("delete max_spid", observed.max_observed_spid, 1031)?;

    Ok(StructuralReplayDeleteCandidate {
        source_hash,
        created_hash: sha256_digest(created_pub),
        output_hash: sha256_digest(&output),
        changed_streams,
        preserved_stream_count,
        all_logical_streams_equal_source,
        bytes: output,
    })
}

fn require_captured_request(
    request: &OrdinaryShapeReplayRequest,
) -> Result<(), StructuralReplayMaterializationBlocked> {
    require_materialized(
        "geometry.left_emu",
        request.geometry.left_emu,
        T406_AUTHORED_LEFT_EMU,
    )?;
    require_materialized(
        "geometry.top_emu",
        request.geometry.top_emu,
        T406_AUTHORED_TOP_EMU,
    )?;
    require_materialized(
        "geometry.width_emu",
        request.geometry.width_emu,
        T406_AUTHORED_WIDTH_EMU,
    )?;
    require_materialized(
        "geometry.height_emu",
        request.geometry.height_emu,
        T406_AUTHORED_HEIGHT_EMU,
    )?;
    require_materialized("fill_rgb", request.fill_rgb, T406_AUTHORED_FILL_RGB)?;
    require_materialized(
        "line_width_emu",
        request.line_width_emu,
        T406_AUTHORED_LINE_WIDTH_EMU,
    )?;
    Ok(())
}

fn require_materialized<T>(
    field: &'static str,
    actual: T,
    expected: T,
) -> Result<(), StructuralReplayMaterializationBlocked>
where
    T: Copy + PartialEq + fmt::Display,
{
    if actual == expected {
        Ok(())
    } else {
        Err(StructuralReplayMaterializationBlocked::RequestMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        })
    }
}

fn require_template_hash(
    path: &'static str,
    bytes: &[u8],
    expected: [u8; 32],
) -> Result<(), StructuralReplayMaterializationBlocked> {
    let expected = Sha256Digest::from_bytes(expected);
    let actual = sha256_digest(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(StructuralReplayMaterializationBlocked::TemplateHashMismatch {
            path,
            expected,
            actual,
        })
    }
}

fn validate_owned_stream_changes(
    before_pub: &[u8],
    after_pub: &[u8],
) -> Result<(Vec<StructuralReplayStreamDelta>, usize), StructuralReplayMaterializationBlocked> {
    let before = logical_stream_map(before_pub)?;
    let after = logical_stream_map(after_pub)?;
    if before.keys().collect::<Vec<_>>() != after.keys().collect::<Vec<_>>() {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: "logical CFB stream topology changed".into(),
        });
    }

    let mut changed = Vec::new();
    let mut preserved = 0usize;
    for (path, (before_len, before_hash)) in before {
        let Some((after_len, after_hash)) = after.get(&path).copied() else {
            return Err(StructuralReplayMaterializationBlocked::OutputValidation {
                detail: format!("output lost logical stream {path}"),
            });
        };
        if before_len == after_len && before_hash == after_hash {
            preserved += 1;
            continue;
        }
        if path != CONTENTS_STREAM_PATH && path != ESCHER_STREAM_PATH {
            return Err(StructuralReplayMaterializationBlocked::UnexpectedStreamMutation {
                path,
            });
        }
        changed.push(StructuralReplayStreamDelta {
            path,
            before_len,
            after_len,
            before_sha256: before_hash,
            after_sha256: after_hash,
        });
    }

    for required in [CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH] {
        if !changed.iter().any(|delta| delta.path == required) {
            return Err(StructuralReplayMaterializationBlocked::MissingOwnedStreamMutation {
                path: required,
            });
        }
    }
    Ok((changed, preserved))
}

fn logical_streams_equal(
    left_pub: &[u8],
    right_pub: &[u8],
) -> Result<bool, StructuralReplayMaterializationBlocked> {
    Ok(logical_stream_map(left_pub)? == logical_stream_map(right_pub)?)
}

fn logical_stream_map(
    pub_bytes: &[u8],
) -> Result<BTreeMap<String, (u64, Sha256Digest)>, StructuralReplayMaterializationBlocked> {
    let inventory = pub_cfb::inspect_reader(Cursor::new(pub_bytes)).map_err(|error| {
        StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        }
    })?;
    let mut streams = BTreeMap::new();
    for entry in inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == pub_cfb::EntryKind::Stream)
    {
        let bytes = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), &entry.path).map_err(
            |error| StructuralReplayMaterializationBlocked::Cfb {
                detail: format!("{error:#}"),
            },
        )?;
        streams.insert(entry.path.clone(), (entry.len, sha256_digest(&bytes)));
    }
    Ok(streams)
}

fn validate_created_candidate(
    output_pub: &[u8],
) -> Result<(), StructuralReplayMaterializationBlocked> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(output_pub), CONTENTS_STREAM_PATH)
        .map_err(|error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        })?;
    require_template_hash(
        CONTENTS_STREAM_PATH,
        &contents,
        T406_NATIVE_CONTENTS_HASH,
    )?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(output_pub), ESCHER_STREAM_PATH).map_err(
        |error| StructuralReplayMaterializationBlocked::Cfb {
            detail: format!("{error:#}"),
        },
    )?;
    require_template_hash(ESCHER_STREAM_PATH, &escher, T406_NATIVE_ESCHER_HASH)?;

    let manifest = build_mature_0x2c_structural_base_manifest(output_pub).map_err(|error| {
        StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!("structural-base manifest failed: {error:#}"),
        }
    })?;
    let matches = manifest
        .candidates
        .iter()
        .filter(|candidate| candidate.contents_seq_num == T406_SHAPE_SEQ_NUM)
        .collect::<Vec<_>>();
    let [shape] = matches.as_slice() else {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!(
                "expected exactly one created seq{}, observed {}",
                T406_SHAPE_SEQ_NUM,
                matches.len()
            ),
        });
    };
    if shape.officeart_spid != T406_NEW_SPID || shape.officeart_shape_type != 1 {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!(
                "created Escher identity mismatch: spid={}, type={}",
                shape.officeart_spid, shape.officeart_shape_type
            ),
        });
    }
    if shape.bounds_emu.x != LengthEmu(T406_PERSISTED_X_EMU)
        || shape.bounds_emu.y != LengthEmu(T406_PERSISTED_Y_EMU)
        || shape.bounds_emu.width != LengthEmu(T406_PERSISTED_WIDTH_EMU)
        || shape.bounds_emu.height != LengthEmu(T406_PERSISTED_HEIGHT_EMU)
    {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!("created bounds mismatch: {:?}", shape.bounds_emu),
        });
    }
    if shape.escher_shape.client_textbox.is_some() {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: "ordinary non-text replay unexpectedly materialized ClientTextbox".into(),
        });
    }
    let client_shape_ids = shape
        .escher_shape
        .client_data
        .as_ref()
        .map(|record| record.values(PUBLISHER_FIELD_SHAPE_ID).collect::<Vec<_>>())
        .unwrap_or_default();
    if client_shape_ids != [T406_SHAPE_SEQ_NUM] {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!("ClientData shape-id mismatch: {client_shape_ids:?}"),
        });
    }

    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(stream, &contents).map_err(|error| {
        StructuralReplayMaterializationBlocked::OutputValidation {
            detail: error.to_string(),
        }
    })?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header).map_err(|error| {
        StructuralReplayMaterializationBlocked::OutputValidation {
            detail: error.to_string(),
        }
    })?;
    require_materialized("created slot_count", trailer.slot_count, 307)?;
    require_materialized("created max_ordinal", trailer.max_ordinal, 306)?;

    let created_reference =
        parse_confirmed_chunk_reference(&contents, &trailer.directory, 305).map_err(|error| {
            StructuralReplayMaterializationBlocked::OutputValidation {
                detail: error.to_string(),
            }
        })?;
    let created_reference =
        created_reference.ok_or_else(|| StructuralReplayMaterializationBlocked::OutputValidation {
            detail: "created seq305 reference is empty".into(),
        })?;
    require_reference(
        &created_reference,
        0x0001,
        T406_PAGE_PARENT_SEQ_NUM,
        9474,
        22,
        "created seq305",
    )?;

    let relocated_reference =
        parse_confirmed_chunk_reference(&contents, &trailer.directory, 306).map_err(|error| {
            StructuralReplayMaterializationBlocked::OutputValidation {
                detail: error.to_string(),
            }
        })?;
    let relocated_reference = relocated_reference.ok_or_else(|| {
        StructuralReplayMaterializationBlocked::OutputValidation {
            detail: "relocated seq306 reference is empty".into(),
        }
    })?;
    require_reference(
        &relocated_reference,
        0x006C,
        T406_SERVICE_PARENT_SEQ_NUM,
        9474,
        1,
        "relocated seq306",
    )?;

    Ok(())
}

fn require_reference(
    reference: &pub_contents::Contents0x2cChunkReference,
    raw_type: u16,
    parent: u32,
    field_06: u16,
    field_0b: u16,
    label: &'static str,
) -> Result<(), StructuralReplayMaterializationBlocked> {
    if unique_observed_u16(&reference.raw_types) != Some(raw_type)
        || unique_observed_u32(&reference.parent_seq_nums) != Some(parent)
        || unique_u16_field(&reference.fields, 0x06) != Some(field_06)
        || unique_u16_field(&reference.fields, 0x0B) != Some(field_0b)
    {
        return Err(StructuralReplayMaterializationBlocked::OutputValidation {
            detail: format!("{label} reference signature mismatch"),
        });
    }
    Ok(())
}

fn unique_observed_u16(fields: &[pub_contents::ObservedU16Field]) -> Option<u16> {
    match fields {
        [field] => Some(field.value),
        _ => None,
    }
}

fn unique_observed_u32(fields: &[pub_contents::ObservedU32Field]) -> Option<u32> {
    match fields {
        [field] => Some(field.value),
        _ => None,
    }
}

fn unique_u16_field(fields: &[pub_contents::RawContentsBlock], id: u16) -> Option<u16> {
    let values = fields
        .iter()
        .filter(|field| field.id == id)
        .filter_map(|field| match &field.body {
            RawContentsBlockBody::U16 { value, .. } => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(*value),
        _ => None,
    }
}

fn unique_u32_field(fields: &[pub_contents::RawContentsBlock], id: u16) -> Option<u32> {
    let values = fields
        .iter()
        .filter(|field| field.id == id)
        .filter_map(|field| match &field.body {
            RawContentsBlockBody::U32 { value, .. } => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(*value),
        _ => None,
    }
}

fn require<T>(
    field: &'static str,
    actual: T,
    expected: T,
) -> Result<(), StructuralReplayPlanBlocked>
where
    T: Copy + PartialEq + fmt::Display,
{
    if actual == expected {
        Ok(())
    } else {
        Err(StructuralReplayPlanBlocked::Precondition {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        })
    }
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

    fn hash(byte: u8) -> Sha256Digest {
        Sha256Digest::from_bytes([byte; 32])
    }

    fn admitted_observation() -> StructuralReplayObservedBase {
        StructuralReplayObservedBase {
            source_hash: hash(7),
            stream_count: 10,
            candidate_count: 6,
            slot_count: 306,
            max_ordinal: 305,
            t370_contents_seq_num: 293,
            t370_parent_seq_num: 266,
            t370_spid: 1025,
            t370_shape_type: 202,
            t370_width_emu: 5_076_000,
            t370_height_emu: 972_000,
            service_seq_num: 305,
            service_raw_type: 0x006C,
            service_parent_seq_num: 282,
            service_field_06: 9474,
            service_field_0b: 1,
            max_observed_spid: 1031,
        }
    }

    fn request() -> OrdinaryShapeReplayRequest {
        OrdinaryShapeReplayRequest {
            expected_source_hash: hash(7),
            geometry: OrdinaryShapeReplayGeometry {
                left_emu: 1_003_300,
                top_emu: 1_435_100,
                width_emu: 2_298_700,
                height_emu: 927_100,
            },
            fill_rgb: 0x0000_A5FF,
            line_width_emu: 28_575,
        }
    }

    #[test]
    fn t406_plan_consumes_the_bounded_native_allocation_law() {
        let plan =
            plan_bounded_t406_ordinary_shape_replay(&admitted_observation(), &request())
                .expect("bounded T406 plan");

        assert_eq!(plan.new_shape_seq_num, 305);
        assert_eq!(plan.relocated_service_seq_num, 306);
        assert_eq!(plan.new_spid, 1032);
        assert_eq!(plan.new_shape_directory_field_06, 9474);
        assert_eq!(plan.new_shape_directory_field_0b, 22);
        assert_eq!(plan.relocated_service_field_0b, 1);
        assert!(!plan.oid_mutation_required);
        assert!(!plan.dw_next_unique_oid_mutation_required);
        assert_eq!(plan.owned_streams, [CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH]);
    }

    #[test]
    fn t406_plan_refuses_allocator_drift_instead_of_guessing() {
        let mut observed = admitted_observation();
        observed.service_seq_num = 304;

        let error = plan_bounded_t406_ordinary_shape_replay(&observed, &request())
            .expect_err("slot drift must fail closed");

        assert_eq!(error.code(), "precondition_mismatch");
        assert!(error.to_string().contains("service_seq_num"));
    }

    #[test]
    fn t406_plan_refuses_spid_drift_instead_of_allocating_heuristically() {
        let mut observed = admitted_observation();
        observed.max_observed_spid = 1037;

        let error = plan_bounded_t406_ordinary_shape_replay(&observed, &request())
            .expect_err("SPID drift must fail closed");

        assert_eq!(error.code(), "precondition_mismatch");
        assert!(error.to_string().contains("max_observed_spid"));
    }
}
