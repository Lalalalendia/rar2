use anyhow::{Context, Result};
use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    inspect_validated_delayed_blips_prefix, validate_blip_record, BlipKind, OfficeArtBody,
    OfficeArtHeader, OfficeArtRecord,
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

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(args.next().context("usage: partial-escherdelay-wmf-reject-probe SOURCE.pub SID")?);
    let sid: u32 = args
        .next()
        .context("usage: partial-escherdelay-wmf-reject-probe SOURCE.pub SID")?
        .to_string_lossy()
        .parse()
        .context("SID must be u32")?;
    if args.next().is_some() {
        anyhow::bail!("partial-escherdelay-wmf-reject-probe accepts SOURCE.pub SID");
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
    let stream = StreamPath(STREAM.into());
    let inventory = inspect_validated_delayed_blips_prefix(stream.clone(), &prefix);

    let mut rejected = Vec::new();
    for value in inventory
        .rejected_complete_blips
        .iter()
        .filter(|value| value.kind == BlipKind::Wmf)
    {
        let offset = usize::try_from(value.record_source.offset).context("offset fits usize")?;
        let ver_instance = u16::from_le_bytes([prefix[offset], prefix[offset + 1]]);
        let rec_type = u16::from_le_bytes([prefix[offset + 2], prefix[offset + 3]]);
        let rec_len = u32::from_le_bytes([
            prefix[offset + 4],
            prefix[offset + 5],
            prefix[offset + 6],
            prefix[offset + 7],
        ]);
        let record_len = 8u64 + u64::from(rec_len);
        let header = OfficeArtHeader {
            rec_ver: (ver_instance & 0x000f) as u8,
            rec_instance: ver_instance >> 4,
            rec_type,
            rec_len,
            source: RawSpan {
                stream: stream.clone(),
                offset: value.record_source.offset,
                len: 8,
            },
        };
        let record = OfficeArtRecord {
            header,
            source: RawSpan {
                stream: stream.clone(),
                offset: value.record_source.offset,
                len: record_len,
            },
            payload_source: RawSpan {
                stream: stream.clone(),
                offset: value.record_source.offset + 8,
                len: u64::from(rec_len),
            },
            sibling_tail_source: None,
            body: OfficeArtBody::Raw,
        };
        let error = validate_blip_record(&prefix, &record)
            .expect_err("inventory marked WMF as strict-validation-failed");
        rejected.push(json!({
            "offset": value.record_source.offset,
            "record_len": record_len,
            "rec_type": format!("0x{rec_type:04x}"),
            "rec_instance": format!("0x{:03x}", ver_instance >> 4),
            "error_display": error.to_string(),
            "error_debug": format!("{error:?}"),
        }));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "source_sha256": source_sha256,
            "stream_sid": sid,
            "available_prefix_len": recovered.available_prefix_len,
            "rejected_wmf": rejected,
        }))?
    );
    Ok(())
}
