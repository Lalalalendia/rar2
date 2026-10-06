use anyhow::{Context, Result, bail};
use pub_cfb::RootRegularStreamPrefixStatus;
use pub_reader::{
    ReaderPartialContentsClass, analyze_reader_partial_contents_prefix,
    build_reader_partial_root_stream_evidence,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "chaptera.partial-root-contents-census.v1";

#[derive(Debug, Serialize)]
struct CensusRow {
    source_sha256: String,
    stream_sid: u32,
    prefix_sha256: String,
    declared_len: u64,
    available_prefix_len: u64,
    missing_tail_len: u64,
    family: Option<String>,
    boundary: String,
    class: String,
    complete_chunk_fact_count: usize,
    fully_decoded_chunk_fact_count: usize,
    raw_type_counts: BTreeMap<String, usize>,
    ambiguous_reference_count: usize,
    referenced_chunk_unavailable_count: usize,
    chunk_parse_failure_count: usize,
    chunk_crosses_trailer_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ContextSemanticSignature {
    available_prefix_len: u64,
    family: Option<String>,
    boundary: String,
    class: String,
    complete_chunk_fact_count: usize,
    fully_decoded_chunk_fact_count: usize,
    raw_type_counts: BTreeMap<String, usize>,
    ambiguous_reference_count: usize,
    referenced_chunk_unavailable_count: usize,
    chunk_parse_failure_count: usize,
    chunk_crosses_trailer_count: usize,
}

#[derive(Debug, Serialize)]
struct CensusContext {
    prefix_sha256: String,
    declared_len: u64,
    source_count: usize,
    semantic: ContextSemanticSignature,
}

#[derive(Debug, Serialize)]
struct ErrorRow {
    source_sha256: String,
    error_signature_sha256: String,
}

#[derive(Debug, Serialize)]
struct CensusSummary {
    schema: &'static str,
    pub_file_count: usize,
    partial_source_count: usize,
    unique_context_count: usize,
    source_class_counts: BTreeMap<String, usize>,
    context_class_counts: BTreeMap<String, usize>,
    rows: Vec<CensusRow>,
    contexts: Vec<CensusContext>,
    non_partial_or_unavailable_count: usize,
    errors: Vec<ErrorRow>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect_pub_paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_pub_paths(&path, out)?;
            continue;
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn boundary_name(value: pub_reader::ReaderPartialContentsBoundary) -> &'static str {
    use pub_reader::ReaderPartialContentsBoundary::*;
    match value {
        EvidenceInvalid => "evidence_invalid",
        FamilyUnrecognized => "family_unrecognized",
        Legacy22SeparateGrammarRequired => "legacy22_separate_grammar_required",
        HeaderOnlyTrailerUnavailable => "header_only_trailer_unavailable",
        TrailerIncomplete => "trailer_incomplete",
        DirectoryComplete => "directory_complete",
        DirectoryWithCompleteReferencedChunks => "directory_with_complete_referenced_chunks",
    }
}

fn class_name(value: ReaderPartialContentsClass) -> &'static str {
    match value {
        ReaderPartialContentsClass::UsefulSemanticPrefix => "useful_semantic_prefix",
        ReaderPartialContentsClass::ForensicOnly => "forensic_only",
        ReaderPartialContentsClass::NoSafeFact => "no_safe_fact",
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: partial-root-contents-census CORPUS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: partial-root-contents-census CORPUS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("partial-root-contents-census accepts exactly CORPUS_DIR OUTPUT.json");
    }

    let mut paths = Vec::new();
    collect_pub_paths(&root, &mut paths)?;
    paths.sort();

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut non_partial_or_unavailable_count = 0usize;
    let mut source_class_counts = BTreeMap::<String, usize>::new();

    for path in &paths {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let source_sha256 = sha256_hex(&bytes);

        let evidence = match build_reader_partial_root_stream_evidence(&bytes, "/Contents") {
            Ok(evidence) => evidence,
            Err(error) => {
                non_partial_or_unavailable_count += 1;
                errors.push(ErrorRow {
                    source_sha256,
                    error_signature_sha256: sha256_hex(format!("{error:#}").as_bytes()),
                });
                continue;
            }
        };

        if evidence.status != RootRegularStreamPrefixStatus::Partial {
            non_partial_or_unavailable_count += 1;
            continue;
        }

        let semantic = analyze_reader_partial_contents_prefix(&evidence);
        let class = class_name(semantic.class).to_owned();
        *source_class_counts.entry(class.clone()).or_default() += 1;

        let fully_decoded_chunk_fact_count = semantic
            .complete_chunk_facts
            .iter()
            .filter(|fact| fact.fully_decoded)
            .count();
        let mut raw_type_counts = BTreeMap::<String, usize>::new();
        for fact in &semantic.complete_chunk_facts {
            *raw_type_counts
                .entry(format!("0x{:04x}", fact.raw_type))
                .or_default() += 1;
        }

        rows.push(CensusRow {
            source_sha256: semantic.source_sha256,
            stream_sid: semantic.stream_sid,
            prefix_sha256: semantic.prefix_sha256,
            declared_len: semantic.declared_len,
            available_prefix_len: semantic.available_prefix_len,
            missing_tail_len: semantic.missing_tail_len,
            family: semantic.family,
            boundary: boundary_name(semantic.boundary).to_owned(),
            class,
            complete_chunk_fact_count: semantic.complete_chunk_facts.len(),
            fully_decoded_chunk_fact_count,
            raw_type_counts,
            ambiguous_reference_count: semantic.ambiguous_reference_count,
            referenced_chunk_unavailable_count: semantic.referenced_chunk_unavailable_count,
            chunk_parse_failure_count: semantic.chunk_parse_failure_count,
            chunk_crosses_trailer_count: semantic.chunk_crosses_trailer_count,
        });
    }

    rows.sort_by(|left, right| {
        left.prefix_sha256
            .cmp(&right.prefix_sha256)
            .then(left.declared_len.cmp(&right.declared_len))
            .then(left.source_sha256.cmp(&right.source_sha256))
    });
    errors.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let mut context_map =
        BTreeMap::<(String, u64), (usize, ContextSemanticSignature)>::new();
    for row in &rows {
        let key = (row.prefix_sha256.clone(), row.declared_len);
        let signature = ContextSemanticSignature {
            available_prefix_len: row.available_prefix_len,
            family: row.family.clone(),
            boundary: row.boundary.clone(),
            class: row.class.clone(),
            complete_chunk_fact_count: row.complete_chunk_fact_count,
            fully_decoded_chunk_fact_count: row.fully_decoded_chunk_fact_count,
            raw_type_counts: row.raw_type_counts.clone(),
            ambiguous_reference_count: row.ambiguous_reference_count,
            referenced_chunk_unavailable_count: row.referenced_chunk_unavailable_count,
            chunk_parse_failure_count: row.chunk_parse_failure_count,
            chunk_crosses_trailer_count: row.chunk_crosses_trailer_count,
        };
        match context_map.get_mut(&key) {
            Some((source_count, existing)) => {
                if *existing != signature {
                    bail!(
                        "same partial prefix context produced conflicting semantic classification: {} / {}",
                        key.0,
                        key.1
                    );
                }
                *source_count += 1;
            }
            None => {
                context_map.insert(key, (1, signature));
            }
        }
    }

    let mut context_class_counts = BTreeMap::<String, usize>::new();
    let contexts = context_map
        .into_iter()
        .map(|((prefix_sha256, declared_len), (source_count, semantic))| {
            *context_class_counts
                .entry(semantic.class.clone())
                .or_default() += 1;
            CensusContext {
                prefix_sha256,
                declared_len,
                source_count,
                semantic,
            }
        })
        .collect::<Vec<_>>();

    let summary = CensusSummary {
        schema: SCHEMA,
        pub_file_count: paths.len(),
        partial_source_count: rows.len(),
        unique_context_count: contexts.len(),
        source_class_counts,
        context_class_counts,
        rows,
        contexts,
        non_partial_or_unavailable_count,
        errors,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&summary).context("serialize partial-root census")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    Ok(())
}
