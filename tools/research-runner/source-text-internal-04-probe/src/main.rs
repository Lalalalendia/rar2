use chaptera_viewer_render_plan::{
    RenderParagraphAlignmentV1, build_page_render_plan_v1,
    complete_scalar_source_font_family_v1,
};
use pub_viewer::ViewerParagraphLineSpacing;
use std::{collections::BTreeMap, env, error::Error, fs};

fn visible_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_whitespace() && ch != '\r')
}

fn bump<K: Ord>(map: &mut BTreeMap<K, usize>, key: K) {
    *map.entry(key).or_default() += 1;
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args()
        .nth(1)
        .ok_or("usage: source-text-internal-04-probe INPUT.pub")?;
    let bytes = fs::read(input)?;
    let visual =
        pub_viewer::open_pub_geometry(&bytes, pub_viewer::viewer_geometry_environment_v0_1())?;

    let plan = build_page_render_plan_v1(&visual, 0)?;

    let mut visible_fragments = 0usize;
    let mut family_authoritative = 0usize;
    let mut family_unresolved = 0usize;
    let mut backend_font_resource_present = 0usize;
    let mut family_counts = BTreeMap::<String, usize>::new();
    let mut text_size_counts = BTreeMap::<u32, usize>::new();
    let mut alignment_counts = BTreeMap::<String, usize>::new();
    let mut page_fragment_ranges = Vec::<(String, u32, u32)>::new();

    for node in plan.nodes {
        let Some(fragment) = node.text.as_ref() else {
            continue;
        };
        if !visible_text(&fragment.text) {
            continue;
        }

        visible_fragments += 1;
        page_fragment_ranges.push((
            format!("{:?}", fragment.story_id),
            fragment.scalar_start,
            fragment.scalar_end,
        ));

        match complete_scalar_source_font_family_v1(fragment) {
            Some(family) => {
                family_authoritative += 1;
                bump(&mut family_counts, family);
            }
            None => family_unresolved += 1,
        }

        if fragment.backend_font_resource_id.is_some() {
            backend_font_resource_present += 1;
        }

        for run in &fragment.typography {
            bump(&mut text_size_counts, run.text_size_emu);
        }

        for run in &fragment.paragraph_alignments {
            let key = match run.alignment {
                RenderParagraphAlignmentV1::Center => "center",
                RenderParagraphAlignmentV1::Right => "right",
                RenderParagraphAlignmentV1::InterWord => "interword",
                RenderParagraphAlignmentV1::Distribute => "distribute",
            };
            bump(&mut alignment_counts, key.to_owned());
        }
    }

    let mut line_spacing_counts = BTreeMap::<String, usize>::new();
    let mut line_spacing_source_values = BTreeMap::<String, usize>::new();
    for run in &visual.paragraph_line_spacings {
        let story_key = format!("{:?}", run.story_id);
        let overlaps_page_fragment = page_fragment_ranges.iter().any(|(story, start, end)| {
            story == &story_key && run.scalar_end > *start && run.scalar_start < *end
        });
        if !overlaps_page_fragment {
            continue;
        }

        let spacing_key = match run.line_spacing {
            ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu,
            } => format!("proportional:{point_equivalent_emu}"),
            ViewerParagraphLineSpacing::Absolute { spacing_emu } => {
                format!("absolute:{spacing_emu}")
            }
        };
        bump(&mut line_spacing_counts, spacing_key);
        bump(
            &mut line_spacing_source_values,
            format!("{:?}", run.source_value),
        );
    }

    eprintln!(
        "TEXT_INTERNAL_04_CENSUS page=1 visible_fragments={} family_authoritative={} family_unresolved={} backend_font_resource_present={} family_counts={:?} text_size_counts={:?} alignment_counts={:?} line_spacing_counts={:?} line_spacing_source_values={:?}",
        visible_fragments,
        family_authoritative,
        family_unresolved,
        backend_font_resource_present,
        family_counts,
        text_size_counts,
        alignment_counts,
        line_spacing_counts,
        line_spacing_source_values,
    );

    Ok(())
}
