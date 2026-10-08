use pub_core::StreamPath;
use std::{collections::BTreeMap, env, error::Error, fs, io::Cursor};

const QUILL_STREAM_PATH: &str = "/Quill/QuillSub/CONTENTS";
const UNDERLINE_FIELD_ID: u16 = 0x001e;

fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args()
        .nth(1)
        .ok_or("usage: quill-underline-carrier-probe INPUT.pub")?;
    let bytes = fs::read(input)?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), QUILL_STREAM_PATH)?;
    let catalog = pub_quill::parse_confirmed_story_catalog(
        StreamPath(QUILL_STREAM_PATH.to_owned()),
        &quill,
    )?;
    let fdpc = pub_quill::inspect_raw_fdpc_styles(&quill, &catalog)?;
    let stsh = pub_quill::inspect_raw_stsh_character_defaults(&quill, &catalog)?;

    let mut fdpc_values = BTreeMap::<String, usize>::new();
    let mut stsh_values = BTreeMap::<String, usize>::new();
    let mut fdpc_style_count = 0usize;
    let mut stsh_style_count = 0usize;

    for style in &fdpc {
        let hits = style
            .properties
            .iter()
            .filter(|property| property.field_id == UNDERLINE_FIELD_ID)
            .collect::<Vec<_>>();
        if !hits.is_empty() {
            fdpc_style_count += 1;
        }
        for property in hits {
            let key = match property.scalar_value {
                Some(value) => format!("scalar:{value}"),
                None => format!("nonscalar:type_{:02x}", property.block_type),
            };
            *fdpc_values.entry(key).or_default() += 1;
        }
    }

    for style in &stsh {
        let hits = style
            .properties
            .iter()
            .filter(|property| property.field_id == UNDERLINE_FIELD_ID)
            .collect::<Vec<_>>();
        if !hits.is_empty() {
            stsh_style_count += 1;
        }
        for property in hits {
            let key = match property.scalar_value {
                Some(value) => format!("scalar:{value}"),
                None => format!("nonscalar:type_{:02x}", property.block_type),
            };
            *stsh_values.entry(key).or_default() += 1;
        }
    }

    let payload = serde_json::json!({
        "schema": "chaptera.quill-underline-carrier-census.v1",
        "field_id": UNDERLINE_FIELD_ID,
        "fdpc": {
            "style_count": fdpc.len(),
            "styles_with_carrier": fdpc_style_count,
            "carrier_count": fdpc_values.values().sum::<usize>(),
            "value_counts": fdpc_values,
        },
        "stsh_character_defaults": {
            "style_count": stsh.len(),
            "styles_with_carrier": stsh_style_count,
            "carrier_count": stsh_values.values().sum::<usize>(),
            "value_counts": stsh_values,
        },
        "claims": {
            "semantic_enum_assigned": false,
            "raw_story_text_emitted": false,
            "raw_offsets_emitted": false,
            "raw_source_bytes_emitted": false,
        }
    });
    println!("QUILL_UNDERLINE_CARRIER_CENSUS {}", serde_json::to_string(&payload)?);
    Ok(())
}
