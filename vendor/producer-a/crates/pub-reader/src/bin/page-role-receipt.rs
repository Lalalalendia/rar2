use anyhow::{Context, Result};
use pub_core::Sha256Digest;
use pub_reader::{analyze_mature_0x2c_page_roles, derive_pub_page_id};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{env, fs, fs::File, io::BufWriter};

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("usage: page-role-receipt SOURCE.pub [OUTPUT.json]")?;
    let output = args.next();

    let bytes = fs::read(&source).with_context(|| format!("read {:?}", source))?;
    let digest = Sha256::digest(&bytes);
    let mut source_hash_bytes = [0_u8; 32];
    source_hash_bytes.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(source_hash_bytes);

    let receipt = analyze_mature_0x2c_page_roles(std::io::Cursor::new(&bytes))?;
    let mut value = serde_json::to_value(&receipt)?;
    let pages = value
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .context("page-role receipt pages must be an array")?;
    if pages.len() != receipt.pages.len() {
        anyhow::bail!("page-role receipt page serialization drift");
    }
    for (json_page, page) in pages.iter_mut().zip(receipt.pages.iter()) {
        let page_id = derive_pub_page_id(&source_hash, page.contents_seq_num)?;
        let fingerprint =
            format!("{:x}", Sha256::digest(page_id.as_canonical().as_bytes()));
        let object = json_page
            .as_object_mut()
            .context("page-role receipt page must be an object")?;
        object.insert(
            "page_identity_fingerprint_sha256".to_owned(),
            Value::String(fingerprint),
        );
    }

    match output {
        Some(path) => {
            let file = File::create(&path).with_context(|| format!("create {:?}", path))?;
            serde_json::to_writer_pretty(BufWriter::new(file), &value)?;
        }
        None => {
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }

    Ok(())
}
