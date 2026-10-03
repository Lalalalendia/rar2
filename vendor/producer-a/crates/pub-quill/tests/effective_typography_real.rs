use pub_core::StreamPath;
use pub_quill::{
    QuillParagraphSelectorSource, QuillTypographyValueSource, parse_bounded_typography,
    parse_confirmed_story_catalog,
};
use std::io::Cursor;
use std::path::Path;

fn effective_counts(path: &Path) -> (usize, usize, usize, usize, Vec<u8>) {
    let pub_bytes = std::fs::read(path).expect("read pinned PUB fixture");
    let quill = pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        "/Quill/QuillSub/CONTENTS",
    )
    .expect("read Quill stream");
    let stories = parse_confirmed_story_catalog(
        StreamPath("/Quill/QuillSub/CONTENTS".into()),
        &quill,
    )
    .expect("parse Story catalog");
    let typography =
        parse_bounded_typography(&quill, &stories).expect("parse bounded effective typography");

    eprintln!(
        "{} typography fences: fdpc_unknown={:?} inheritance_unknown={:?} inheritance_reason={:?}",
        path.display(),
        typography.unknown_block_types_assumed_zero_length,
        typography.inheritance_unknown_block_types_assumed_zero_length,
        typography.effective_inheritance_unavailable_reason,
    );
    assert!(
        typography.effective_inheritance_unavailable_reason.is_none(),
        "effective inheritance must be available: {:?}",
        typography.effective_inheritance_unavailable_reason
    );
    assert!(
        typography
            .inheritance_unknown_block_types_assumed_zero_length
            .is_empty(),
        "real acceptance must not promote through unknown inheritance block widths"
    );

    let explicit_complete = typography
        .effective_runs
        .iter()
        .filter(|run| {
            run.font_source == QuillTypographyValueSource::ExplicitFdpc
                && run.text_size_source == QuillTypographyValueSource::ExplicitFdpc
        })
        .count();
    let inherited_explicit_selector = typography
        .effective_runs
        .iter()
        .filter(|run| {
            run.uses_inheritance()
                && run.inherited_selector_source
                    == Some(QuillParagraphSelectorSource::ExplicitFdpp0x19)
        })
        .count();
    let inherited_implicit_zero = typography
        .effective_runs
        .iter()
        .filter(|run| {
            run.uses_inheritance()
                && run.inherited_selector_source
                    == Some(
                        QuillParagraphSelectorSource::ImplicitStyleZeroFromBoundedEvidence,
                    )
        })
        .count();

    (
        typography.effective_runs.len(),
        explicit_complete,
        inherited_explicit_selector,
        inherited_implicit_zero,
        typography.unknown_block_types_assumed_zero_length,
    )
}

#[test]
#[ignore = "requires pinned Apache POI SampleNewsletter and SampleBrochure paths"]
fn real_pub_effective_typography_matches_product_authority_and_brochure_fence() {
    let newsletter = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER");
    let brochure = std::env::var_os("CHAPTERA_SAMPLE_BROCHURE")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_SAMPLE_BROCHURE");

    let newsletter_counts = effective_counts(&newsletter);
    let brochure_counts = effective_counts(&brochure);

    eprintln!(
        "SampleNewsletter effective={} explicit={} inherited_explicit_selector={} inherited_implicit_zero={}",
        newsletter_counts.0,
        newsletter_counts.1,
        newsletter_counts.2,
        newsletter_counts.3,
    );
    eprintln!(
        "SampleNewsletter FDPC unknown fixed block types: {:?}",
        newsletter_counts.4
    );
    eprintln!(
        "SampleBrochure effective={} explicit={} inherited_explicit_selector={} inherited_implicit_zero={}",
        brochure_counts.0,
        brochure_counts.1,
        brochure_counts.2,
        brochure_counts.3,
    );
    eprintln!(
        "SampleBrochure FDPC unknown fixed block types: {:?}",
        brochure_counts.4
    );

    assert_eq!(newsletter_counts, (106, 18, 88, 0, Vec::new()));
    assert_eq!(brochure_counts, (67, 13, 54, 0, Vec::new()));
}

fn alignment_source_class(
    stories: &pub_quill::QuillStoryCatalog,
    source: &pub_core::RawSpan,
) -> &'static str {
    let source_start = source.offset;
    let source_end = source.offset.saturating_add(source.len);
    for descriptor in stories
        .descriptor_nodes
        .iter()
        .flat_map(|node| node.descriptors.iter())
    {
        let chunk_start = u64::from(descriptor.data_offset.value);
        let chunk_end = chunk_start.saturating_add(u64::from(descriptor.data_length.value));
        if chunk_start <= source_start && source_end <= chunk_end {
            return match descriptor.name.value {
                [b'F', b'D', b'P', b'P'] => "explicit_fdpp",
                [b'S', b'T', b'S', b'H'] => "inherited_stsh",
                _ => "other",
            };
        }
    }
    "unclassified"
}

#[test]
#[ignore = "requires the pinned public Carlton March PUB path"]
fn real_carlton_paragraph_alignment_receipt_is_source_safe() {
    let carlton = std::env::var_os("CHAPTERA_GOLDEN_CARLTON_MARCH")
        .map(std::path::PathBuf::from)
        .expect("CHAPTERA_GOLDEN_CARLTON_MARCH");
    let pub_bytes = std::fs::read(&carlton).expect("read pinned Carlton PUB fixture");
    let quill = pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        "/Quill/QuillSub/CONTENTS",
    )
    .expect("read Carlton Quill stream");
    let stories = parse_confirmed_story_catalog(
        StreamPath("/Quill/QuillSub/CONTENTS".into()),
        &quill,
    )
    .expect("parse Carlton Story catalog");
    let typography =
        parse_bounded_typography(&quill, &stories).expect("parse Carlton bounded typography");

    assert!(
        typography.effective_inheritance_unavailable_reason.is_none(),
        "Carlton paragraph inheritance must not cross an unresolved physical fence: {:?}",
        typography.effective_inheritance_unavailable_reason
    );

    let mut explicit = 0_usize;
    let mut inherited = 0_usize;
    let mut other = 0_usize;

    eprintln!("CARLTON_PARAGRAPH_ALIGNMENT_RECEIPT_BEGIN");
    for (ordinal, run) in typography.paragraph_alignments.iter().enumerate() {
        let source_class = alignment_source_class(&stories, &run.fdpp_style_source);
        match source_class {
            "explicit_fdpp" => explicit += 1,
            "inherited_stsh" => inherited += 1,
            _ => other += 1,
        }
        eprintln!(
            "alignment_run ordinal={} story_index={} utf16_len={} alignment={:?} source_value={} source_class={}",
            ordinal,
            run.story_index,
            run.story_end_utf16.saturating_sub(run.story_start_utf16),
            run.alignment,
            run.source_value,
            source_class,
        );
    }
    eprintln!(
        "alignment_summary total={} explicit_fdpp={} inherited_stsh={} other={}",
        typography.paragraph_alignments.len(),
        explicit,
        inherited,
        other,
    );
    eprintln!("CARLTON_PARAGRAPH_ALIGNMENT_RECEIPT_END");

    assert_eq!(
        explicit + inherited + other,
        typography.paragraph_alignments.len()
    );
    assert_eq!(
        other, 0,
        "every admitted Carlton alignment run must retain bounded FDPP/STSH provenance"
    );
}

