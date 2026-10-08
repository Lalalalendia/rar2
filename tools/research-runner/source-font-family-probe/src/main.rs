use chaptera_viewer_render_plan::{
    RenderTextFragmentV1, build_page_render_plan_v1, complete_scalar_source_font_family_v1,
    effective_source_font_family_v1,
};
use pub_viewer::{ViewerGeometryDocument, ViewerScriptFontEntryDisposition};
use std::{collections::BTreeMap, env, error::Error, fs};

fn visible_text(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_whitespace() && ch != '\r')
}

fn normalize_family(name: &str) -> String {
    name.trim().to_lowercase()
}

fn scalar_failure_reason(fragment: &RenderTextFragmentV1) -> &'static str {
    if fragment.typography.is_empty() {
        return "scalar_absent";
    }

    let mut cursor = fragment.scalar_start;
    let mut family: Option<String> = None;
    let mut saw_blank = false;

    for run in &fragment.typography {
        if run.scalar_start != cursor || run.scalar_end <= run.scalar_start {
            return "scalar_coverage_gap";
        }
        if run.scalar_end > fragment.scalar_end {
            return "scalar_range_invalid";
        }

        let display = run.source_font_name.trim();
        if display.is_empty() {
            if family.is_some() {
                return "scalar_blank_mixed_with_family";
            }
            saw_blank = true;
        } else {
            if saw_blank {
                return "scalar_blank_mixed_with_family";
            }
            let normalized = normalize_family(display);
            match family.as_ref() {
                None => family = Some(normalized),
                Some(existing) if *existing == normalized => {}
                Some(_) => return "scalar_mixed_family",
            }
        }
        cursor = run.scalar_end;
    }

    if cursor != fragment.scalar_end {
        return "scalar_coverage_gap";
    }

    match (family.is_some(), saw_blank) {
        (false, true) => "scalar_absent",
        (true, false) => "scalar_authoritative_unexpected",
        _ => "scalar_invalid",
    }
}

fn one_resolved_script_entry<'a>(
    entries: &'a [pub_viewer::ViewerScriptFontEntry],
    slot: u16,
) -> Result<&'a pub_viewer::ViewerScriptFontEntry, &'static str> {
    let matches = entries
        .iter()
        .filter(|entry| entry.script_slot == slot)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("script_slot_missing_or_duplicate");
    }
    let entry = matches[0];
    if entry.disposition != ViewerScriptFontEntryDisposition::Resolved {
        return Err("script_entry_unresolved");
    }
    if entry
        .source_font_name
        .as_deref()
        .is_none_or(|name| name.trim().is_empty())
    {
        return Err("script_family_blank");
    }
    Ok(entry)
}

fn script_failure_reason(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> &'static str {
    if fragment.text.is_empty() {
        return "script_empty_fragment";
    }
    if !fragment.text.is_ascii() {
        return "script_non_ascii_fragment";
    }

    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)
    else {
        return "script_story_missing";
    };

    let mut maps = visual
        .script_font_maps
        .iter()
        .filter(|map| map.story_id == fragment.story_id)
        .filter(|map| map.applies_to_story_text(&story.text))
        .filter(|map| {
            map.scalar_end > fragment.scalar_start && map.scalar_start < fragment.scalar_end
        })
        .collect::<Vec<_>>();
    maps.sort_by_key(|map| (map.scalar_start, map.scalar_end));
    if maps.is_empty() {
        return "script_no_overlapping_maps";
    }

    let mut cursor = fragment.scalar_start;
    let mut selected: Option<(u32, String)> = None;

    for map in maps {
        let start = map.scalar_start.max(fragment.scalar_start);
        let end = map.scalar_end.min(fragment.scalar_end);
        if start != cursor || end <= start {
            return "script_coverage_gap";
        }

        let default = match one_resolved_script_entry(&map.entries, 0) {
            Ok(value) => value,
            Err(reason) => return reason,
        };
        let ascii_latin = match one_resolved_script_entry(&map.entries, 1) {
            Ok(value) => value,
            Err(reason) => return reason,
        };
        let latin = match one_resolved_script_entry(&map.entries, 2) {
            Ok(value) => value,
            Err(reason) => return reason,
        };

        let entries = [default, ascii_latin, latin];
        let first_name = normalize_family(
            entries[0]
                .source_font_name
                .as_deref()
                .expect("resolved entry has family"),
        );
        let first = (entries[0].source_font_index, first_name);
        if entries.iter().skip(1).any(|entry| {
            entry.source_font_index != first.0
                || normalize_family(
                    entry
                        .source_font_name
                        .as_deref()
                        .expect("resolved entry has family"),
                ) != first.1
        }) {
            return "script_slot_family_mixed";
        }

        match selected.as_ref() {
            None => selected = Some(first),
            Some(existing) if *existing == first => {}
            Some(_) => return "script_range_family_mixed",
        }
        cursor = end;
    }

    if cursor != fragment.scalar_end {
        return "script_coverage_gap";
    }
    "script_authoritative_unexpected"
}

fn unresolved_reason(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> &'static str {
    let scalar_reason = scalar_failure_reason(fragment);
    if scalar_reason != "scalar_absent" {
        return scalar_reason;
    }
    script_failure_reason(visual, fragment)
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
    let mut unresolved_reasons = BTreeMap::<String, usize>::new();

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
            let (authority, family, reason) = match (scalar, effective) {
                (Some(family), _) => {
                    scalar_authoritative += 1;
                    ("scalar", Some(family), "scalar_authoritative")
                }
                (None, Some(family)) => {
                    script_authoritative += 1;
                    ("script_fonts", Some(family), "script_authoritative")
                }
                (None, None) => {
                    unresolved += 1;
                    let reason = unresolved_reason(&visual, fragment);
                    *unresolved_reasons.entry(reason.to_owned()).or_default() += 1;
                    ("unresolved", None, reason)
                }
            };

            if let Some(family) = family.as_ref() {
                *family_counts.entry(family.clone()).or_default() += 1;
            }
            eprintln!(
                "SOURCE_FONT_FRAGMENT label={label:?} page={} scalar_start={} scalar_end={} authority={} reason={} typography_run_count={}",
                page_index + 1,
                fragment.scalar_start,
                fragment.scalar_end,
                authority,
                reason,
                fragment.typography.len(),
            );
        }
    }

    eprintln!(
        "SOURCE_FONT_FAMILY_CENSUS label={label:?} visible_fragments={total} scalar_authoritative={scalar_authoritative} script_authoritative={script_authoritative} unresolved={unresolved} unresolved_reasons={unresolved_reasons:?} family_counts={family_counts:?}"
    );
    Ok(())
}
