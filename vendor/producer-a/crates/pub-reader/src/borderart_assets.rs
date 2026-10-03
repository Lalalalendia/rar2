use anyhow::{Context, Result, bail};
use pub_contents::{
    BLOCK_TYPE_BINARY, BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_90, BLOCK_TYPE_CONTAINER_A0,
    BLOCK_TYPE_U16, BLOCK_TYPE_U32, ContentsCursor, RawContentsBlock, RawContentsBlockBody,
    parse_confirmed_block,
};
use pub_core::RawSpan;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor};

use crate::wmf::validate_wmf_metafile;
use crate::{
    PubBorderArtCatalogDiagnosticV1, PubBorderArtCatalogEntryV1, PubBorderArtShapeUseV1,
    read_mature_0x2c_borderart_catalog_v1,
};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const OPLFB_DZL_CORNER_FIELD: u16 = 0x04;
const OPLFB_DXL_HORIZ_FIELD: u16 = 0x05;
const OPLFB_DYL_VERT_FIELD: u16 = 0x06;
const OPLFB_CMETA_FIELD: u16 = 0x07;
const OPLFB_RGFBMETA_FIELD: u16 = 0x08;
const OPLFB_CFBMD_FIELD: u16 = 0x09;
const OPLFB_RGFBMD_FIELD: u16 = 0x0A;
const OPLFBMD_RGBMETA_FIELD: u16 = 0x01;
const BORDERART_SLOT_COUNT: usize = 8;
const BORDERART_RESOURCE_POOL_BASE: u32 = 108;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubBorderArtSlotV1 {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl PubBorderArtSlotV1 {
    pub const ORDERED: [Self; BORDERART_SLOT_COUNT] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtWmfResourceV1 {
    pub pool_index: u8,
    pub pool_offset: u32,
    pub sha256: String,
    pub source: RawSpan,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtSlotRefV1 {
    pub slot: PubBorderArtSlotV1,
    pub resource_pool_index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtAssetEntryV1 {
    pub ordinal: u32,
    pub name: String,
    pub corner_extent_emu: u32,
    pub horizontal_extent_emu: u32,
    pub vertical_extent_emu: u32,
    pub resources: Vec<PubBorderArtWmfResourceV1>,
    pub slots: Vec<PubBorderArtSlotRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubBorderArtAssetReadV1 {
    pub ifbmax: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<PubBorderArtAssetEntryV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_uses: Vec<PubBorderArtShapeUseV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PubBorderArtCatalogDiagnosticV1>,
}

fn span_slice<'a>(contents: &'a [u8], source: &RawSpan) -> Result<&'a [u8]> {
    let start = usize::try_from(source.offset).context("BorderArt span start")?;
    let len = usize::try_from(source.len).context("BorderArt span length")?;
    let end = start
        .checked_add(len)
        .context("BorderArt span end overflow")?;
    contents
        .get(start..end)
        .context("BorderArt span crosses Contents boundary")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn exact_u32(block: &RawContentsBlock, field_id: u16) -> Option<u32> {
    if block.id != field_id || block.block_type != BLOCK_TYPE_U32 {
        return None;
    }
    match block.body {
        RawContentsBlockBody::U32 { value, .. } => Some(value),
        _ => None,
    }
}

fn exact_u16(block: &RawContentsBlock, field_id: u16) -> Option<u16> {
    if block.id != field_id || block.block_type != BLOCK_TYPE_U16 {
        return None;
    }
    match block.body {
        RawContentsBlockBody::U16 { value, .. } => Some(value),
        _ => None,
    }
}

fn container_source(block: &RawContentsBlock, field_id: u16, wire: u8) -> Option<RawSpan> {
    if block.id != field_id || block.block_type != wire {
        return None;
    }
    match &block.body {
        RawContentsBlockBody::Container { content_source, .. } => Some(content_source.clone()),
        _ => None,
    }
}

fn parse_metadata_offsets(contents: &[u8], source: &RawSpan) -> Result<Vec<u16>> {
    let start = usize::try_from(source.offset).context("BorderArt metadata start")?;
    let len = usize::try_from(source.len).context("BorderArt metadata length")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)?;
    let mut offsets = Vec::new();

    while cursor.remaining() > 0 {
        let block =
            parse_confirmed_block(&mut cursor).context("parse BorderArt metadata offset")?;
        if block.id != 0 || block.block_type != BLOCK_TYPE_U16 {
            bail!(
                "BorderArt metadata child uses field0x{:03X}/wire0x{:02X}, expected field0/wire0x{BLOCK_TYPE_U16:02X}",
                block.id,
                block.block_type
            );
        }
        let RawContentsBlockBody::U16 { value, .. } = block.body else {
            bail!("BorderArt metadata offset has non-U16 body");
        };
        offsets.push(value);
    }
    Ok(offsets)
}

fn parse_resource_pool(
    contents: &[u8],
    source: &RawSpan,
) -> Result<Vec<PubBorderArtWmfResourceV1>> {
    let start = usize::try_from(source.offset).context("BorderArt resource pool start")?;
    let len = usize::try_from(source.len).context("BorderArt resource pool length")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)?;
    let mut resources = Vec::new();
    let mut pool_offset = BORDERART_RESOURCE_POOL_BASE;

    while cursor.remaining() > 0 {
        let child = parse_confirmed_block(&mut cursor).context("parse BorderArt OplFbmd child")?;
        if child.block_type != BLOCK_TYPE_CONTAINER_88 {
            bail!(
                "BorderArt resource child uses wire0x{:02X}, expected 0x{BLOCK_TYPE_CONTAINER_88:02X}",
                child.block_type
            );
        }
        let RawContentsBlockBody::Container { content_source, .. } = child.body else {
            bail!("BorderArt resource child is not a bounded container");
        };

        let child_start =
            usize::try_from(content_source.offset).context("BorderArt OplFbmd content start")?;
        let child_len =
            usize::try_from(content_source.len).context("BorderArt OplFbmd content length")?;
        let mut child_cursor = ContentsCursor::bounded(
            content_source.stream.clone(),
            contents,
            child_start,
            child_len,
        )?;
        let blob = parse_confirmed_block(&mut child_cursor).context("parse BorderArt RgbMeta")?;
        if blob.id != OPLFBMD_RGBMETA_FIELD || blob.block_type != BLOCK_TYPE_BINARY {
            bail!(
                "BorderArt RgbMeta uses field0x{:03X}/wire0x{:02X}, expected field0x001/wire0x{BLOCK_TYPE_BINARY:02X}",
                blob.id,
                blob.block_type
            );
        }
        if child_cursor.remaining() != 0 {
            bail!("BorderArt OplFbmd contains bytes after RgbMeta");
        }
        let RawContentsBlockBody::Binary { value_source, .. } = blob.body else {
            bail!("BorderArt RgbMeta has non-binary body");
        };
        let bytes = span_slice(contents, &value_source)?.to_vec();
        validate_wmf_metafile(&bytes).context("validate BorderArt WMF")?;

        let pool_index =
            u8::try_from(resources.len()).context("BorderArt resource pool exceeds u8")?;
        resources.push(PubBorderArtWmfResourceV1 {
            pool_index,
            pool_offset,
            sha256: sha256_hex(&bytes),
            source: value_source,
            bytes,
        });
        let payload_len = u32::try_from(
            resources
                .last()
                .expect("just pushed BorderArt resource")
                .bytes
                .len(),
        )
        .context("BorderArt WMF length exceeds u32")?;
        pool_offset = pool_offset
            .checked_add(payload_len)
            .context("BorderArt resource pool offset overflow")?;
    }

    if resources.is_empty() || resources.len() > BORDERART_SLOT_COUNT {
        bail!(
            "BorderArt resource pool cardinality {} is outside 1..={BORDERART_SLOT_COUNT}",
            resources.len()
        );
    }
    Ok(resources)
}

fn resolve_slot_refs(
    metadata_offsets: &[u16],
    resources: &[PubBorderArtWmfResourceV1],
) -> Result<Vec<PubBorderArtSlotRefV1>> {
    if metadata_offsets.len() != BORDERART_SLOT_COUNT {
        bail!(
            "BorderArt metadata exposes {} semantic slots, expected {BORDERART_SLOT_COUNT}",
            metadata_offsets.len()
        );
    }
    if metadata_offsets.first().copied() != Some(BORDERART_RESOURCE_POOL_BASE as u16) {
        bail!(
            "BorderArt first semantic slot offset is {:?}, expected {BORDERART_RESOURCE_POOL_BASE}",
            metadata_offsets.first()
        );
    }

    let starts = resources
        .iter()
        .map(|resource| (resource.pool_offset, resource.pool_index))
        .collect::<BTreeMap<_, _>>();

    PubBorderArtSlotV1::ORDERED
        .iter()
        .zip(metadata_offsets)
        .map(|(slot, offset)| {
            let pool_index = starts
                .get(&u32::from(*offset))
                .copied()
                .with_context(|| {
                    format!(
                        "BorderArt semantic slot {slot:?} offset {offset} does not resolve to a resource-pool start"
                    )
                })?;
            Ok(PubBorderArtSlotRefV1 {
                slot: *slot,
                resource_pool_index: pool_index,
            })
        })
        .collect()
}

fn parse_asset_entry(
    contents: &[u8],
    entry: &PubBorderArtCatalogEntryV1,
) -> Result<PubBorderArtAssetEntryV1> {
    let child_start =
        usize::try_from(entry.source.offset).context("BorderArt OplFb source start")?;
    let child_len = usize::try_from(entry.source.len).context("BorderArt OplFb source length")?;
    let mut child_cursor = ContentsCursor::bounded(
        entry.source.stream.clone(),
        contents,
        child_start,
        child_len,
    )?;
    let child =
        parse_confirmed_block(&mut child_cursor).context("reparse BorderArt OplFb child")?;
    if child.block_type != BLOCK_TYPE_CONTAINER_88 || child_cursor.remaining() != 0 {
        bail!("BorderArt catalog entry source is not one exact OplFb wire0x88 child");
    }
    let RawContentsBlockBody::Container { content_source, .. } = child.body else {
        bail!("BorderArt OplFb child is not a bounded container");
    };

    let content_end = content_source
        .offset
        .checked_add(content_source.len)
        .context("BorderArt OplFb content end overflow")?;
    let after_name = entry
        .name_source
        .offset
        .checked_add(entry.name_source.len)
        .context("BorderArt name end overflow")?;
    if after_name > content_end {
        bail!("BorderArt catalog name crosses OplFb boundary");
    }

    let start = usize::try_from(after_name).context("BorderArt post-name start")?;
    let len = usize::try_from(content_end - after_name).context("BorderArt post-name length")?;
    let mut cursor = ContentsCursor::bounded(content_source.stream.clone(), contents, start, len)?;

    let mut corner_extent_emu = None;
    let mut horizontal_extent_emu = None;
    let mut vertical_extent_emu = None;
    let mut cmeta = None;
    let mut metadata_source = None;
    let mut cfbmd = None;
    let mut resources_source = None;

    while cursor.remaining() > 0 {
        let block = parse_confirmed_block(&mut cursor).context("parse BorderArt OplFb field")?;
        if let Some(value) = exact_u32(&block, OPLFB_DZL_CORNER_FIELD) {
            corner_extent_emu = Some(value);
        } else if let Some(value) = exact_u32(&block, OPLFB_DXL_HORIZ_FIELD) {
            horizontal_extent_emu = Some(value);
        } else if let Some(value) = exact_u32(&block, OPLFB_DYL_VERT_FIELD) {
            vertical_extent_emu = Some(value);
        } else if let Some(value) = exact_u32(&block, OPLFB_CMETA_FIELD) {
            cmeta = Some(value);
        } else if let Some(source) =
            container_source(&block, OPLFB_RGFBMETA_FIELD, BLOCK_TYPE_CONTAINER_90)
        {
            metadata_source = Some(source);
        } else if let Some(value) = exact_u16(&block, OPLFB_CFBMD_FIELD) {
            cfbmd = Some(value);
        } else if let Some(source) =
            container_source(&block, OPLFB_RGFBMD_FIELD, BLOCK_TYPE_CONTAINER_A0)
        {
            resources_source = Some(source);
        }
    }

    if cmeta != Some(BORDERART_SLOT_COUNT as u32) {
        bail!(
            "BorderArt CMeta is {:?}, expected {BORDERART_SLOT_COUNT}",
            cmeta
        );
    }
    let metadata_source = metadata_source.context("BorderArt entry missing RgfbMeta")?;
    let resources_source = resources_source.context("BorderArt entry missing RgFbmd")?;
    let resources = parse_resource_pool(contents, &resources_source)?;
    if cfbmd.map(usize::from) != Some(resources.len()) {
        bail!(
            "BorderArt CFbmd={cfbmd:?} does not match resource-pool cardinality {}",
            resources.len()
        );
    }
    let metadata_offsets = parse_metadata_offsets(contents, &metadata_source)?;
    let slots = resolve_slot_refs(&metadata_offsets, &resources)?;

    Ok(PubBorderArtAssetEntryV1 {
        ordinal: entry.ordinal,
        name: entry.name.clone(),
        corner_extent_emu: corner_extent_emu.context("BorderArt entry missing DzlCorner")?,
        horizontal_extent_emu: horizontal_extent_emu.context("BorderArt entry missing DxlHoriz")?,
        vertical_extent_emu: vertical_extent_emu.context("BorderArt entry missing DylVert")?,
        resources,
        slots,
    })
}

pub fn read_mature_0x2c_borderart_assets_from_pub_bytes_v1(
    pub_bytes: &[u8],
) -> Result<PubBorderArtAssetReadV1> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(pub_bytes), CONTENTS_STREAM_PATH)
        .context("read /Contents for BorderArt asset projection")?;
    read_mature_0x2c_borderart_assets_v1(&contents)
}

pub fn read_mature_0x2c_borderart_assets_v1(contents: &[u8]) -> Result<PubBorderArtAssetReadV1> {
    let catalog_read = read_mature_0x2c_borderart_catalog_v1(contents)?;
    let mut diagnostics = catalog_read.diagnostics;
    let shape_uses = catalog_read.shape_uses;
    let ifbmax = catalog_read.catalog.as_ref().map(|catalog| catalog.ifbmax);
    let mut entries = Vec::new();

    if let Some(catalog) = catalog_read.catalog {
        for entry in &catalog.entries {
            match parse_asset_entry(contents, entry) {
                Ok(asset) => entries.push(asset),
                Err(error) => diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                    code: "borderart_catalog_asset_unavailable".into(),
                    contents_seq_num: None,
                    detail: format!(
                        "catalog ordinal {} ({:?}) rejected: {error:#}",
                        entry.ordinal, entry.name
                    ),
                }),
            }
        }
        if entries.len() != catalog.entries.len() {
            diagnostics.push(PubBorderArtCatalogDiagnosticV1 {
                code: "borderart_catalog_asset_incomplete".into(),
                contents_seq_num: None,
                detail: format!(
                    "{} of {} catalog entries have complete geometry/resource projection",
                    entries.len(),
                    catalog.entries.len()
                ),
            });
        }
    }

    Ok(PubBorderArtAssetReadV1 {
        ifbmax,
        entries,
        shape_uses,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(pool_index: u8, pool_offset: u32, len: usize) -> PubBorderArtWmfResourceV1 {
        PubBorderArtWmfResourceV1 {
            pool_index,
            pool_offset,
            sha256: String::new(),
            source: RawSpan {
                stream: pub_core::StreamPath("/Contents".into()),
                offset: 0,
                len: len as u64,
            },
            bytes: vec![0; len],
        }
    }

    #[test]
    fn slot_refs_allow_single_resource_reused_by_all_directions() {
        let resources = vec![resource(0, 108, 16)];
        let offsets = [108u16; 8];
        let slots = resolve_slot_refs(&offsets, &resources).expect("single resource reuse");
        assert_eq!(slots.len(), 8);
        assert!(slots.iter().all(|slot| slot.resource_pool_index == 0));
        assert_eq!(slots[0].slot, PubBorderArtSlotV1::TopLeft);
        assert_eq!(slots[7].slot, PubBorderArtSlotV1::Left);
    }

    #[test]
    fn slot_refs_allow_partial_dedup_resource_pool() {
        let resources = vec![
            resource(0, 108, 20),
            resource(1, 128, 30),
            resource(2, 158, 40),
            resource(3, 198, 50),
        ];
        let offsets = [108, 128, 158, 198, 108, 128, 158, 198];
        let slots = resolve_slot_refs(&offsets, &resources).expect("partial dedup");
        assert_eq!(
            slots
                .iter()
                .map(|slot| slot.resource_pool_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 0, 1, 2, 3]
        );
    }

    #[test]
    fn slot_refs_preserve_full_eight_resource_order() {
        let resources = (0u8..8)
            .map(|index| resource(index, 108 + u32::from(index) * 10, 10))
            .collect::<Vec<_>>();
        let offsets = [108, 118, 128, 138, 148, 158, 168, 178];
        let slots = resolve_slot_refs(&offsets, &resources).expect("eight resources");
        assert_eq!(
            slots
                .iter()
                .map(|slot| slot.resource_pool_index)
                .collect::<Vec<_>>(),
            (0u8..8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn slot_refs_reject_unresolved_offset() {
        let resources = vec![resource(0, 108, 16)];
        let offsets = [108, 108, 108, 109, 108, 108, 108, 108];
        let error = resolve_slot_refs(&offsets, &resources)
            .expect_err("non-pool-start offset must fail closed");
        assert!(error.to_string().contains("does not resolve"));
    }

    #[test]
    fn slot_refs_require_eight_semantic_directions() {
        let resources = vec![resource(0, 108, 16)];
        let error = resolve_slot_refs(&[108; 7], &resources)
            .expect_err("seven semantic offsets must fail closed");
        assert!(error.to_string().contains("expected 8"));
    }
}
