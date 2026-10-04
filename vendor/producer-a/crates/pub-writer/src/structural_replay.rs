use pub_contents::{
    RawContentsBlockBody, parse_0x2c_header, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::inspect_sp_containers;
use pub_model::Sha256Digest;
use pub_reader::{
    CONTENTS_STREAM_PATH, ESCHER_STREAM_PATH, build_mature_0x2c_structural_base_manifest,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Cursor;

pub const STRUCT_WRITER_REPLAY_SCHEMA_V0_1: &str = "pub-struct-writer-replay-plan/v0.1";

pub const T406_SHAPE_SEQ_NUM: u32 = 305;
pub const T406_RELOCATED_SERVICE_SEQ_NUM: u32 = 306;
pub const T406_PAGE_PARENT_SEQ_NUM: u32 = 266;
pub const T406_SERVICE_PARENT_SEQ_NUM: u32 = 282;
pub const T406_NEW_SPID: u32 = 1032;

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
