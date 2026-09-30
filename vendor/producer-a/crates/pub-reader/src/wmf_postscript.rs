use crate::wmf::bounded_wmf_metafile;
use anyhow::{Context, Result, bail};

const PLACEABLE_HEADER_BYTES: usize = 22;
const META_HEADER_BYTES: usize = 18;
const META_EOF: u16 = 0x0000;
const META_ESCAPE: u16 = 0x0626;
const POSTSCRIPT_DATA: u16 = 0x0025;
const MAX_POSTSCRIPT_CHUNKS: usize = 16;
const MAX_POSTSCRIPT_CHUNK_BYTES: usize = 8 * 1024;
const MAX_POSTSCRIPT_PROGRAM_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmfPostScriptProgram {
    pub bytes: Vec<u8>,
    pub chunk_count: usize,
    pub declared_payload_bytes: usize,
    pub padded_chunk_count: usize,
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

pub fn extract_bounded_wmf_postscript_program(
    source: &[u8],
) -> Result<Option<WmfPostScriptProgram>> {
    let bounded = bounded_wmf_metafile(source).context("WMF is not structurally bounded")?;
    let bytes = &bounded.normalized_bytes;
    let header_offset = if bounded.info.placeable {
        PLACEABLE_HEADER_BYTES
    } else {
        0
    };
    let mut offset = header_offset
        .checked_add(META_HEADER_BYTES)
        .context("WMF record offset overflow")?;
    let mut program = Vec::new();
    let mut chunk_count = 0usize;
    let mut declared_payload_bytes = 0usize;
    let mut padded_chunk_count = 0usize;

    while offset < bytes.len() {
        let record_words = usize::try_from(
            read_u32(bytes, offset).context("WMF record size is truncated")?,
        )
        .context("WMF record size overflows usize")?;
        let record_bytes = record_words
            .checked_mul(2)
            .context("WMF record byte size overflow")?;
        let record_end = offset
            .checked_add(record_bytes)
            .context("WMF record range overflow")?;
        if record_end > bytes.len() || record_bytes < 6 {
            bail!("WMF record exceeds bounded payload");
        }

        let function =
            read_u16(bytes, offset + 4).context("WMF record function is truncated")?;
        let params = &bytes[offset + 6..record_end];

        if function == META_ESCAPE
            && read_u16(params, 0) == Some(POSTSCRIPT_DATA)
        {
            if params.len() < 4 {
                bail!("WMF POSTSCRIPT_DATA header is truncated");
            }
            if chunk_count >= MAX_POSTSCRIPT_CHUNKS {
                bail!("WMF POSTSCRIPT_DATA chunk count exceeds bounded limit");
            }

            let byte_count = usize::from(
                read_u16(params, 2).context("WMF POSTSCRIPT_DATA ByteCount is truncated")?,
            );
            if byte_count == 0 || byte_count > MAX_POSTSCRIPT_CHUNK_BYTES {
                bail!("WMF POSTSCRIPT_DATA chunk length is outside bounded limit");
            }

            let stored_payload = &params[4..];
            if stored_payload.len() < byte_count || stored_payload.len() > byte_count + 1 {
                bail!("WMF POSTSCRIPT_DATA stored payload length is invalid");
            }
            if stored_payload.len() == byte_count + 1 {
                if stored_payload[byte_count] != 0 {
                    bail!("WMF POSTSCRIPT_DATA alignment pad must be zero");
                }
                padded_chunk_count += 1;
            }

            let next_total = declared_payload_bytes
                .checked_add(byte_count)
                .context("WMF POSTSCRIPT_DATA total length overflow")?;
            if next_total > MAX_POSTSCRIPT_PROGRAM_BYTES {
                bail!("WMF POSTSCRIPT_DATA program exceeds bounded limit");
            }
            program.extend_from_slice(&stored_payload[..byte_count]);
            declared_payload_bytes = next_total;
            chunk_count += 1;
        }

        offset = record_end;
        if function == META_EOF {
            break;
        }
    }

    if chunk_count == 0 {
        return Ok(None);
    }

    Ok(Some(WmfPostScriptProgram {
        bytes: program,
        chunk_count,
        declared_payload_bytes,
        padded_chunk_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(function: u16, params: &[u8]) -> Vec<u8> {
        let record_len = 6 + params.len();
        assert_eq!(record_len % 2, 0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(record_len / 2).unwrap().to_le_bytes());
        bytes.extend_from_slice(&function.to_le_bytes());
        bytes.extend_from_slice(params);
        bytes
    }

    fn escape_postscript(payload: &[u8], with_padding: bool) -> Vec<u8> {
        let mut params = Vec::new();
        params.extend_from_slice(&POSTSCRIPT_DATA.to_le_bytes());
        params.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        params.extend_from_slice(payload);
        if with_padding {
            params.push(0);
        }
        record(META_ESCAPE, &params)
    }

    fn wmf_with_records(records: Vec<Vec<u8>>) -> Vec<u8> {
        let mut body = Vec::new();
        for record in records {
            body.extend(record);
        }
        body.extend(record(META_EOF, &[]));

        let total_len = META_HEADER_BYTES + body.len();
        let max_record_words = body
            .chunks(2)
            .count()
            .max(3);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&9_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0300_u16.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(total_len / 2).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(max_record_words).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend(body);

        // Replace MaxRecord with the actual largest record size in words.
        let mut offset = META_HEADER_BYTES;
        let mut max_words = 3_u32;
        while offset < bytes.len() {
            let words = read_u32(&bytes, offset).unwrap();
            max_words = max_words.max(words);
            offset += usize::try_from(words).unwrap() * 2;
        }
        bytes[12..16].copy_from_slice(&max_words.to_le_bytes());
        bytes
    }

    #[test]
    fn extracts_declared_postscript_bytes_across_chunks() {
        let bytes = wmf_with_records(vec![
            escape_postscript(b"abc", true),
            escape_postscript(b"defg", false),
        ]);

        let program = extract_bounded_wmf_postscript_program(&bytes)
            .expect("bounded extraction")
            .expect("PostScript program");
        assert_eq!(program.bytes, b"abcdefg");
        assert_eq!(program.chunk_count, 2);
        assert_eq!(program.declared_payload_bytes, 7);
        assert_eq!(program.padded_chunk_count, 1);
    }

    #[test]
    fn rejects_nonzero_alignment_padding() {
        let mut params = Vec::new();
        params.extend_from_slice(&POSTSCRIPT_DATA.to_le_bytes());
        params.extend_from_slice(&3_u16.to_le_bytes());
        params.extend_from_slice(b"abc");
        params.push(0x7f);
        let bytes = wmf_with_records(vec![record(META_ESCAPE, &params)]);
        assert!(extract_bounded_wmf_postscript_program(&bytes).is_err());
    }

    #[test]
    fn rejects_bytecount_overrun() {
        let mut params = Vec::new();
        params.extend_from_slice(&POSTSCRIPT_DATA.to_le_bytes());
        params.extend_from_slice(&5_u16.to_le_bytes());
        params.extend_from_slice(b"abc");
        params.push(0);
        let bytes = wmf_with_records(vec![record(META_ESCAPE, &params)]);
        assert!(extract_bounded_wmf_postscript_program(&bytes).is_err());
    }

    #[test]
    fn returns_none_without_postscript_data() {
        let bytes = wmf_with_records(vec![record(0x0103, &8_u16.to_le_bytes())]);
        assert_eq!(
            extract_bounded_wmf_postscript_program(&bytes).unwrap(),
            None
        );
    }
}
