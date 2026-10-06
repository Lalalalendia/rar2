use super::*;
use std::io::{Cursor, Write};

fn nested_regular_fixture() -> (Vec<u8>, Vec<u8>) {
    let mut compound =
        cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("nested fixture CFB");
    compound.create_storage("/Escher").expect("Escher storage");
    let expected = vec![0x6b; 9_000];
    compound
        .create_stream("/Escher/EscherDelayStm")
        .expect("nested delay stream")
        .write_all(&expected)
        .expect("write nested delay stream");
    compound.flush().expect("flush nested fixture");
    (compound.into_inner().into_inner(), expected)
}

fn directory_sector_ids(source: &[u8]) -> (usize, Vec<u32>) {
    let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
    let num_sectors = source.len() / sector_len - 1;
    let num_fat_sectors =
        u32::from_le_bytes([source[44], source[45], source[46], source[47]]) as usize;
    let first_directory_sector =
        u32::from_le_bytes([source[48], source[49], source[50], source[51]]);
    let fat = read_fat(source, sector_len, num_sectors, num_fat_sectors).expect("fixture FAT");
    let directory_sector_ids =
        fat_chain_to_end(first_directory_sector, &fat, num_sectors, "fixture directory")
            .expect("fixture directory chain");
    (sector_len, directory_sector_ids)
}

fn raw_directory_entry_offset(source: &[u8], sid: u32) -> usize {
    let (sector_len, directory_sector_ids) = directory_sector_ids(source);
    let logical_offset = usize::try_from(sid).expect("SID usize") * DIR_ENTRY_LEN;
    let directory_sector_ordinal = logical_offset / sector_len;
    let within_sector = logical_offset % sector_len;
    let directory_sector = directory_sector_ids[directory_sector_ordinal];
    (usize::try_from(directory_sector).expect("directory sector") + 1) * sector_len + within_sector
}

fn patch_directory_u32(source: &mut [u8], sid: u32, field_offset: usize, value: u32) {
    let raw_offset = raw_directory_entry_offset(source, sid) + field_offset;
    source[raw_offset..raw_offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn patch_directory_name(source: &mut [u8], sid: u32, new_name: &str) {
    let raw_offset = raw_directory_entry_offset(source, sid);
    let entry = &mut source[raw_offset..raw_offset + DIR_ENTRY_LEN];

    let mut encoded = new_name.encode_utf16().collect::<Vec<_>>();
    assert!(encoded.len() <= 31);
    encoded.push(0);
    entry[..64].fill(0);
    for (index, unit) in encoded.iter().enumerate() {
        entry[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    let byte_len = u16::try_from(encoded.len() * 2).expect("directory name length");
    entry[64..66].copy_from_slice(&byte_len.to_le_bytes());
}

fn inventory(source: &[u8]) -> PhysicalDirectoryInventory {
    inspect_partial_cfb_physical_directory_reader(Cursor::new(source.to_vec()))
        .expect("physical directory inventory")
}

fn sid_by_name(source: &[u8], name: &str, object_type: u8) -> u32 {
    inventory(source)
        .entries
        .into_iter()
        .find(|entry| entry.object_type == object_type && entry.name == name)
        .unwrap_or_else(|| panic!("missing directory entry {name}"))
        .sid
}

fn corrupt_first_fat_link_for_stream(source: &mut [u8], stream_sid: u32) {
    let inv = inventory(source);
    let entry = inv
        .entries
        .iter()
        .find(|entry| entry.sid == stream_sid)
        .expect("stream entry");
    let start_sector = entry.start_sector;
    let sector_len = 1usize << u16::from_le_bytes([source[30], source[31]]);
    let fat_sector = u32::from_le_bytes([source[76], source[77], source[78], source[79]]);
    let fat_offset = (usize::try_from(fat_sector).expect("FAT sector") + 1) * sector_len;
    let entry_offset = fat_offset + usize::try_from(start_sector).expect("stream sector") * 4;
    source[entry_offset..entry_offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
}

#[test]
fn physical_directory_inventory_preserves_exact_nested_sids() {
    let (source, _) = nested_regular_fixture();
    let inv = inventory(&source);
    let escher_sid = sid_by_name(&source, "Escher", 1);
    let delay_sid = sid_by_name(&source, "EscherDelayStm", 2);

    assert_eq!(inv.source_sha256, sha256_hex(&source));
    assert_eq!(inv.source_byte_len, source.len() as u64);
    assert!(inv.entries.iter().any(|entry| {
        entry.sid == escher_sid && entry.object_type == 1 && entry.name == "Escher"
    }));
    assert!(inv.entries.iter().any(|entry| {
        entry.sid == delay_sid
            && entry.object_type == 2
            && entry.name == "EscherDelayStm"
            && entry.declared_len == 9_000
    }));
}

#[test]
fn nested_path_discovery_binds_sid_and_exact_stream_read() {
    let (source, expected_bytes) = nested_regular_fixture();
    let expected_sid = sid_by_name(&source, "EscherDelayStm", 2);
    let discovered = discover_regular_stream_sid_reader(
        Cursor::new(source.clone()),
        "/Escher/EscherDelayStm",
    )
    .expect("discover nested stream SID");

    assert_eq!(discovered.source_sha256, sha256_hex(&source));
    assert_eq!(discovered.source_byte_len, source.len() as u64);
    assert_eq!(discovered.stream_sid, expected_sid);
    assert_eq!(discovered.descriptive_name, "EscherDelayStm");
    assert_eq!(discovered.declared_len, expected_bytes.len() as u64);

    let recovered = recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
        Cursor::new(source),
        discovered.stream_sid,
        &discovered.source_sha256,
    )
    .expect("read discovered SID under same source identity");
    assert_eq!(recovered.bytes, expected_bytes);
}

#[test]
fn discovery_does_not_require_target_stream_chain_to_be_valid() {
    let (mut source, _) = nested_regular_fixture();
    let delay_sid = sid_by_name(&source, "EscherDelayStm", 2);
    corrupt_first_fat_link_for_stream(&mut source, delay_sid);

    let discovered = discover_regular_stream_sid_reader(
        Cursor::new(source.clone()),
        "/Escher/EscherDelayStm",
    )
    .expect("directory discovery must not consume target stream chain");
    assert_eq!(discovered.stream_sid, delay_sid);

    let recovered = recover_regular_stream_prefix_by_sid_reader_with_expected_sha(
        Cursor::new(source),
        delay_sid,
        &discovered.source_sha256,
    )
    .expect("exact SID recovery should retain physically proven prefix");
    assert_eq!(recovered.status, RootRegularStreamPrefixStatus::Partial);
    assert_eq!(
        recovered.truncation_reason,
        Some(RootRegularStreamTruncationReason::InvalidNextSector)
    );
    assert!(recovered.available_prefix_len > 0);
    assert!(recovered.available_prefix_len < recovered.declared_len);
}

#[test]
fn duplicate_case_colliding_child_component_fails_closed() {
    let mut compound =
        cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("nested collision CFB");
    compound.create_storage("/Escher").expect("Escher storage");
    compound
        .create_stream("/Escher/EscherDelayStm")
        .expect("first delay")
        .write_all(&vec![0x11; 9_000])
        .expect("write first delay");
    compound
        .create_stream("/Escher/EscherDelayStn")
        .expect("second delay staging")
        .write_all(&vec![0x22; 9_000])
        .expect("write second delay");
    compound.flush().expect("flush nested collision CFB");
    let mut source = compound.into_inner().into_inner();

    let second_sid = sid_by_name(&source, "EscherDelayStn", 2);
    patch_directory_name(&mut source, second_sid, "ESCHERDELAYSTM");

    let error = discover_regular_stream_sid_reader(
        Cursor::new(source),
        "/Escher/EscherDelayStm",
    )
    .expect_err("case-colliding nested candidates must fail closed");
    assert!(format!("{error:#}").contains("ambiguous case-insensitive directory component"));
}

#[test]
fn nested_sibling_cycle_fails_closed() {
    let (mut source, _) = nested_regular_fixture();
    let delay_sid = sid_by_name(&source, "EscherDelayStm", 2);
    patch_directory_u32(&mut source, delay_sid, 68, delay_sid);

    let error = discover_regular_stream_sid_reader(
        Cursor::new(source),
        "/Escher/EscherDelayStm",
    )
    .expect_err("nested sibling cycle must fail closed");
    assert!(format!("{error:#}").contains("cycle in directory sibling tree"));
}

#[test]
fn storage_child_cycle_fails_closed() {
    let (mut source, _) = nested_regular_fixture();
    let escher_sid = sid_by_name(&source, "Escher", 1);
    patch_directory_u32(&mut source, escher_sid, 76, escher_sid);

    discover_regular_stream_sid_reader(Cursor::new(source), "/Escher/EscherDelayStm")
        .expect_err("storage child cycle must fail closed");
}

#[test]
fn out_of_range_nested_child_sid_fails_closed() {
    let (mut source, _) = nested_regular_fixture();
    let escher_sid = sid_by_name(&source, "Escher", 1);
    patch_directory_u32(&mut source, escher_sid, 76, u32::MAX - 1);

    let error = discover_regular_stream_sid_reader(
        Cursor::new(source),
        "/Escher/EscherDelayStm",
    )
    .expect_err("out-of-range child SID must fail closed");
    assert!(format!("{error:#}").contains("out-of-range SID"));
}

#[test]
fn same_named_streams_under_distinct_storages_remain_distinct() {
    let mut compound =
        cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("same-name CFB");
    compound.create_storage("/A").expect("A storage");
    compound.create_storage("/B").expect("B storage");
    let a_bytes = vec![0x31; 9_000];
    let b_bytes = vec![0x72; 9_000];
    compound
        .create_stream("/A/SameStream")
        .expect("A stream")
        .write_all(&a_bytes)
        .expect("write A");
    compound
        .create_stream("/B/SameStream")
        .expect("B stream")
        .write_all(&b_bytes)
        .expect("write B");
    compound.flush().expect("flush same-name CFB");
    let source = compound.into_inner().into_inner();

    let a = discover_regular_stream_sid_reader(Cursor::new(source.clone()), "/A/SameStream")
        .expect("discover A/SameStream");
    let b = discover_regular_stream_sid_reader(Cursor::new(source.clone()), "/B/SameStream")
        .expect("discover B/SameStream");
    assert_ne!(a.stream_sid, b.stream_sid);

    let a_recovered =
        recover_regular_stream_prefix_by_sid_reader(Cursor::new(source.clone()), a.stream_sid)
            .expect("recover A");
    let b_recovered =
        recover_regular_stream_prefix_by_sid_reader(Cursor::new(source), b.stream_sid)
            .expect("recover B");
    assert_eq!(a_recovered.bytes, a_bytes);
    assert_eq!(b_recovered.bytes, b_bytes);
}
