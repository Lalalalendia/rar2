use anyhow::{Context, Result};
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use std::{env, fs, path::PathBuf};

fn main() -> Result<()> {
    let source = PathBuf::from(
        env::args_os()
            .nth(1)
            .context("usage: scriptfont-effective-probe SOURCE.pub")?,
    );
    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let visual = open_pub_geometry(&bytes, viewer_geometry_environment_v0_1())
        .context("open PUB geometry")?;

    let stories = visual
        .document
        .stories
        .iter()
        .enumerate()
        .map(|(story_index, story)| {
            let maps = visual
                .script_font_maps
                .iter()
                .filter(|map| map.story_id == story.id)
                .map(|map| {
                    let entries = map
                        .entries
                        .iter()
                        .map(|entry| json!({
                            "slot": entry.script_slot,
                            "ordinal": entry.source_font_index,
                            "name": entry.source_font_name,
                            "disposition": format!("{:?}", entry.disposition),
                        }))
                        .collect::<Vec<_>>();
                    json!({
                        "start": map.scalar_start,
                        "end": map.scalar_end,
                        "story_fingerprint_matches": map.applies_to_story_text(&story.text),
                        "entries": entries,
                    })
                })
                .collect::<Vec<_>>();
            let fragments = visual
                .text_fragments
                .iter()
                .filter(|fragment| fragment.story_id == story.id)
                .map(|fragment| json!({
                    "start": fragment.scalar_start,
                    "end": fragment.scalar_end,
                    "ascii": fragment.text.is_ascii(),
                    "chars": fragment.text.chars().count(),
                    "line_count": fragment.line_count,
                }))
                .collect::<Vec<_>>();
            let typography = visual
                .typography_runs
                .iter()
                .filter(|run| run.story_id == story.id)
                .map(|run| json!({
                    "start": run.scalar_start,
                    "end": run.scalar_end,
                    "source_font_name": run.source_font_name,
                    "size_emu": run.text_size_emu,
                    "font_inherited": run.font_inherited,
                    "size_inherited": run.size_inherited,
                    "story_fingerprint_matches": run.applies_to_story_text(&story.text),
                }))
                .collect::<Vec<_>>();
            json!({
                "story_index": story_index,
                "story_chars": story.text.chars().count(),
                "script_font_maps": maps,
                "fragments": fragments,
                "typography": typography,
            })
        })
        .collect::<Vec<_>>();

    println!("{}", serde_json::to_string_pretty(&json!({
        "schema": "chaptera.scriptfont-effective-probe.v1",
        "story_count": visual.document.stories.len(),
        "script_font_map_count": visual.script_font_maps.len(),
        "fragment_count": visual.text_fragments.len(),
        "typography_run_count": visual.typography_runs.len(),
        "stories": stories,
    }))?);
    Ok(())
}
