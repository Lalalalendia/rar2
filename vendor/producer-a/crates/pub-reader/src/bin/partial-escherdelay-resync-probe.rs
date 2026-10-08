use anyhow::{Context, Result};
use pub_core::StreamPath;
use pub_escher::{
    inspect_validated_delayed_blips_prefix, OFFICE_ART_BLIP_DIB, OFFICE_ART_BLIP_EMF,
    OFFICE_ART_BLIP_JPEG, OFFICE_ART_BLIP_PICT, OFFICE_ART_BLIP_PNG, OFFICE_ART_BLIP_TIFF,
    OFFICE_ART_BLIP_WMF,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor, path::PathBuf};

const STREAM: &str = "/Escher/EscherDelayStm";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_blip_type(value: u16) -> bool {
    matches!(
        value,
        OFFICE_ART_BLIP_EMF
            | OFFICE_ART_BLIP_WMF
            | OFFICE_ART_BLIP_PICT
            | OFFICE_ART_BLIP_JPEG
            | OFFICE_ART_BLIP_PNG
            | OFFICE_ART_BLIP_DIB
            | OFFICE_ART_BLIP_TIFF
    )
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(args.next().context("usage: partial-escherdelay-resync-probe SOURCE.pub SID")?);
    let sid: u32 = args
        .next()
        .context("usage: partial-escherdelay-resync-probe SOURCE.pub SID")?
        .to_string_lossy()
        .parse()
        .context("SID must be u32")?;
    if args.next().is_some() {
        anyhow::bail!("partial-escherdelay-resync-probe accepts SOURCE.pub SID");
    }

    let source_bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&source_bytes);
    let recovered =
        pub_cfb::recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
            Cursor::new(&source_bytes),
            sid,
            &source_sha256,
        )
        .context("recover exact truncated EscherDelay prefix")?;
    let prefix = recovered.bytes;

    let mut candidates = Vec::new();
    for offset in 1..prefix.len().saturating_sub(8) {
        let rec_type = u16::from_le_bytes([prefix[offset + 2], prefix[offset + 3]]);
        if !is_blip_type(rec_type) {
            continue;
        }
        let rec_len = u32::from_le_bytes([
            prefix[offset + 4],
            prefix[offset + 5],
            prefix[offset + 6],
            prefix[offset + 7],
        ]) as usize;
        let Some(end) = offset.checked_add(8).and_then(|value| value.checked_add(rec_len)) else {
            continue;
        };
        if end > prefix.len() {
            continue;
        }

        let inventory = inspect_validated_delayed_blips_prefix(
            StreamPath(STREAM.into()),
            &prefix[offset..],
        );
        let Some(first) = inventory.records.first() else {
            continue;
        };
        if first.record_source.offset != 0 {
            continue;
        }

        candidates.push(json!({
            "offset": offset,
            "rec_type": format!("0x{rec_type:04x}"),
            "rec_len": rec_len,
            "kind": format!("{:?}", first.kind).to_ascii_lowercase(),
            "strict_records_from_offset": inventory.records.len(),
            "scanned_records_from_offset": inventory.scanned_record_count,
            "first_record_len": first.record_source.len,
            "first_payload_sha256": first.payload_sha256,
        }));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "source_sha256": source_sha256,
            "stream_sid": sid,
            "declared_len": recovered.declared_len,
            "available_prefix_len": recovered.available_prefix_len,
            "prefix_sha256": recovered.prefix_sha256,
            "strict_resync_candidates": candidates,
        }))?
    );
    Ok(())
}
