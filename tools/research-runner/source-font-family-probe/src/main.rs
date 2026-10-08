use chaptera_viewer_render_plan::{
    build_page_render_plan_v1, complete_scalar_source_font_family_v1,
    effective_source_font_family_v1,
};
use std::{collections::BTreeMap, env, error::Error, fs};

fn visible_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_whitespace() && ch != '\r')
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: source_font_family_census INPUT.pub LABEL")?;
    let label = args
        .next()
        .ok_or("usage: source_font_family_census INPUT.pub LABEL")?;

    let bytes = fs::read(input)?;
    let visual =
        pub_viewer::open_pub_geometry(&bytes, pub_viewer::viewer_geometry_environment_v0_1())?;

    let mut total = 0usize;
    let mut scalar_authoritative = 0usize;
    let mut script_authoritative = 0usize;
    let mut unresolved = 0usize;
    let mut family_counts = BTreeMap::<String, usize>::new();

    for page_index in 0..visual.document.pages.len() {
        let plan = build_page_render_plan_v1(&visual, page_index)?;
        for node in plan.nodes {
            let Some(fragment) = node.text.as_ref() else {
                continue;
            };
            if !visible_text(&fragment.text) {
                continue;
            }
            total += 1;

            let scalar = complete_scalar_source_font_family_v1(fragment);
            let effective = effective_source_font_family_v1(&visual, fragment);
            let (authority, family) = match (scalar, effective) {
                (Some(family), _) => {
                    scalar_authoritative += 1;
                    ("scalar", Some(family))
                }
                (None, Some(family)) => {
                    script_authoritative += 1;
                    ("script_fonts", Some(family))
                }
                (None, None) => {
                    unresolved += 1;
                    ("unresolved", None)
                }
            };

            if let Some(family) = family.as_ref() {
                *family_counts.entry(family.clone()).or_default() += 1;
            }
            eprintln!(
                "SOURCE_FONT_FRAGMENT label={label:?} page={} node_id={} scalar_start={} scalar_end={} authority={} family={:?} typography_run_count={}",
                page_index + 1,
                node.node_id.as_canonical(),
                fragment.scalar_start,
                fragment.scalar_end,
                authority,
                family,
                fragment.typography.len(),
            );
        }
    }

    eprintln!(
        "SOURCE_FONT_FAMILY_CENSUS label={label:?} visible_fragments={total} scalar_authoritative={scalar_authoritative} script_authoritative={script_authoritative} unresolved={unresolved} family_counts={family_counts:?}"
    );
    Ok(())
}
