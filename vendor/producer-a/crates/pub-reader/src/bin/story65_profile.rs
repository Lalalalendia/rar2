use anyhow::{Context, Result};
use pub_cfb::read_stream_path;
use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, Contents0x2cChunkReference, ContentsFamily, RawContentsBlock,
    RawContentsBlockBody, StoryCatalogReadError, detect_family, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_mature_story_catalog,
};
use pub_core::StreamPath;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn one_raw_type(reference: &Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn body_kind(body: &RawContentsBlockBody) -> &'static str {
    match body {
        RawContentsBlockBody::Empty => "empty",
        RawContentsBlockBody::U16 { .. } => "u16",
        RawContentsBlockBody::U32 { .. } => "u32",
        RawContentsBlockBody::Fixed8 { .. } => "fixed8",
        RawContentsBlockBody::Fixed16 { .. } => "fixed16",
        RawContentsBlockBody::Container { .. } => "container",
    }
}

fn field_receipt(field: &RawContentsBlock) -> Value {
    let container_declared_length = match &field.body {
        RawContentsBlockBody::Container {
            declared_length, ..
        } => Some(*declared_length),
        _ => None,
    };
    json!({
        "id": field.id,
        "block_type": field.block_type,
        "body_kind": body_kind(&field.body),
        "source_offset": field.source.offset,
        "source_len": field.source.len,
        "container_declared_length": container_declared_length,
    })
}

fn story_error_kind(error: &StoryCatalogReadError) -> &'static str {
    match error {
        StoryCatalogReadError::Contents(_) => "contents",
        StoryCatalogReadError::Block(_) => "block",
        StoryCatalogReadError::SpanTooLarge { .. } => "span_too_large",
        StoryCatalogReadError::MissingDeclaredCount => "missing_declared_count",
        StoryCatalogReadError::DuplicateDeclaredCount => "duplicate_declared_count",
        StoryCatalogReadError::InvalidDeclaredCount => "invalid_declared_count",
        StoryCatalogReadError::MissingEntryArray => "missing_entry_array",
        StoryCatalogReadError::DuplicateEntryArray => "duplicate_entry_array",
        StoryCatalogReadError::InvalidEntryArray => "invalid_entry_array",
        StoryCatalogReadError::UnexpectedEntryId { .. } => "unexpected_entry_id",
        StoryCatalogReadError::InvalidEntryContainer { .. } => "invalid_entry_container",
        StoryCatalogReadError::MissingTextId { .. } => "missing_text_id",
        StoryCatalogReadError::DuplicateTextId { .. } => "duplicate_text_id",
        StoryCatalogReadError::InvalidTextId { .. } => "invalid_text_id",
        StoryCatalogReadError::MissingLayoutKey { .. } => "missing_layout_key",
        StoryCatalogReadError::DuplicateLayoutKey { .. } => "duplicate_layout_key",
        StoryCatalogReadError::InvalidLayoutKey { .. } => "invalid_layout_key",
        StoryCatalogReadError::DuplicateTextIdentity { .. } => "duplicate_text_identity",
        StoryCatalogReadError::EntryCountMismatch { .. } => "entry_count_mismatch",
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: story65_profile SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: story65_profile SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("story65_profile accepts exactly SOURCE.pub OUTPUT.json");
    }

    let source_bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&source_bytes);

    let contents = match read_stream_path(&source, "/Contents") {
        Ok(contents) => contents,
        Err(_) => {
            fs::write(
                &output,
                serde_json::to_vec_pretty(&json!({
                    "schema": "chaptera.story65-physical-profile.v1",
                    "source_sha256": source_sha256,
                    "byte_len": source_bytes.len(),
                    "status": "contents_unavailable",
                    "story_catalogs": [],
                }))?,
            )?;
            return Ok(());
        }
    };

    let family = match detect_family(&contents) {
        Ok(ContentsFamily::Family0x2c) => "0x2c",
        Ok(ContentsFamily::Family0x22) => "0x22",
        Err(_) => "unknown",
    };
    if family != "0x2c" {
        fs::write(
            &output,
            serde_json::to_vec_pretty(&json!({
                "schema": "chaptera.story65-physical-profile.v1",
                "source_sha256": source_sha256,
                "byte_len": source_bytes.len(),
                "contents_family": family,
                "status": "not_mature_0x2c",
                "story_catalogs": [],
            }))?,
        )?;
        return Ok(());
    }

    let stream = StreamPath("/Contents".to_owned());
    let header = match parse_0x2c_header(stream.clone(), &contents) {
        Ok(header) => header,
        Err(_) => {
            fs::write(
                &output,
                serde_json::to_vec_pretty(&json!({
                    "schema": "chaptera.story65-physical-profile.v1",
                    "source_sha256": source_sha256,
                    "byte_len": source_bytes.len(),
                    "contents_family": family,
                    "status": "header_parse_failed",
                    "story_catalogs": [],
                }))?,
            )?;
            return Ok(());
        }
    };
    let trailer = match parse_confirmed_0x2c_trailer_root(&contents, &header) {
        Ok(trailer) => trailer,
        Err(_) => {
            fs::write(
                &output,
                serde_json::to_vec_pretty(&json!({
                    "schema": "chaptera.story65-physical-profile.v1",
                    "source_sha256": source_sha256,
                    "byte_len": source_bytes.len(),
                    "contents_family": family,
                    "status": "trailer_parse_failed",
                    "story_catalogs": [],
                }))?,
            )?;
            return Ok(());
        }
    };

    let mut catalogs = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let reference =
            match parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num) {
                Ok(Some(reference)) => reference,
                Ok(None) => continue,
                Err(_) => continue,
            };
        if one_raw_type(&reference) != Some(CONTENTS_RAW_TYPE_STORY_CATALOG) {
            continue;
        }

        let offsets = reference
            .chunk_offsets
            .iter()
            .map(|field| field.value)
            .collect::<Vec<_>>();
        if offsets.len() != 1 {
            catalogs.push(json!({
                "seq_num": seq_num,
                "status": "non_unique_chunk_offset",
                "chunk_offset_count": offsets.len(),
            }));
            continue;
        }

        let chunk = match parse_confirmed_0x2c_chunk(stream.clone(), &contents, offsets[0]) {
            Ok(chunk) => chunk,
            Err(_) => {
                catalogs.push(json!({
                    "seq_num": seq_num,
                    "status": "chunk_parse_failed",
                }));
                continue;
            }
        };

        let strict = match parse_confirmed_mature_story_catalog(&contents, &chunk) {
            Ok(catalog) => json!({
                "status": "ok",
                "declared_count": catalog.declared_count,
                "entry_count": catalog.entries.len(),
            }),
            Err(error) => json!({
                "status": "error",
                "error_kind": story_error_kind(&error),
            }),
        };

        catalogs.push(json!({
            "seq_num": seq_num,
            "status": "profiled",
            "chunk_offset": offsets[0],
            "declared_length": chunk.declared_length,
            "top_level_field_count": chunk.fields.len(),
            "top_level_fields": chunk.fields.iter().map(field_receipt).collect::<Vec<_>>(),
            "unsupported_tail": chunk.unsupported_tail.as_ref().map(|source| json!({
                "offset": source.offset,
                "len": source.len,
            })),
            "strict_catalog": strict,
        }));
    }

    let receipt = json!({
        "schema": "chaptera.story65-physical-profile.v1",
        "source_sha256": source_sha256,
        "byte_len": source_bytes.len(),
        "contents_family": family,
        "serialization_revision": header.preamble.serialization_revision,
        "status": "ok",
        "story_catalog_count": catalogs.len(),
        "story_catalogs": catalogs,
    });
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}
