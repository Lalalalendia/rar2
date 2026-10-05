use pub_editor::{
    AuthoredParagraphAlignmentValueV1, EditorEditableTarget, Sha256Digest, open_mature_0x2c_editor,
};
use pub_export::ParagraphAlignmentV1;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "chaptera.paragraph-scoped-alignment-consumer-fixture.v1";

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn non_whitespace_sha256(text: &str) -> String {
    let normalized = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scalar_slice(text: &str, start: u64, end: u64) -> Result<String, Box<dyn Error>> {
    let start = usize::try_from(start)?;
    let end = usize::try_from(end)?;
    let scalar_len = text.chars().count();
    if start > end || end > scalar_len {
        return Err(
            format!("invalid scalar range {start}..{end} for Story length {scalar_len}").into(),
        );
    }
    Ok(text.chars().skip(start).take(end - start).collect())
}

fn write_target(
    session: &pub_editor::EditorSession,
    target: EditorEditableTarget,
    state_root: &Path,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let export = session.export_editable(target, label.to_owned())?;
    fs::write(
        state_root.join(format!("output.{}", target.extension())),
        &export.bytes,
    )?;
    Ok(())
}

fn write_state(
    session: &pub_editor::EditorSession,
    root: &Path,
    state: &str,
) -> Result<(), Box<dyn Error>> {
    let state_root = root.join(state);
    fs::create_dir_all(&state_root)?;
    write_target(
        session,
        EditorEditableTarget::Idml,
        &state_root,
        &format!("carlton-paragraph-scoped-{state}"),
    )?;
    write_target(
        session,
        EditorEditableTarget::Odg,
        &state_root,
        &format!("carlton-paragraph-scoped-{state}"),
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: paragraph_scoped_alignment_consumer_fixture INPUT OUTPUT_DIR")?,
    );
    let output = PathBuf::from(args.next().ok_or("OUTPUT_DIR is required")?);
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }

    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let mut session = open_mature_0x2c_editor(&bytes, hash)?;

    let source_alignments = session.effective_full_story_paragraph_alignment_v1()?;
    let imported = session.imported_paragraphs_v1()?;

    let mut selected = None;
    for alignment in source_alignments
        .iter()
        .filter(|item| item.alignment == ParagraphAlignmentV1::Right)
    {
        let mut paragraphs = imported
            .iter()
            .filter(|paragraph| paragraph.story_id == alignment.story_id)
            .cloned()
            .collect::<Vec<_>>();
        paragraphs.sort_by_key(|paragraph| {
            (
                paragraph.range.start,
                paragraph.range.end,
                paragraph.paragraph_id,
            )
        });
        if paragraphs.len() < 3 {
            continue;
        }
        let Some(story) = session.graph().stories.get(&alignment.story_id) else {
            continue;
        };
        let all_content_bearing = paragraphs.iter().all(|paragraph| {
            scalar_slice(&story.text, paragraph.range.start, paragraph.range.end)
                .ok()
                .is_some_and(|text| text.chars().any(|character| !character.is_whitespace()))
        });
        if all_content_bearing {
            selected = Some((alignment.story_id, paragraphs));
            break;
        }
    }

    let (story_id, paragraphs) = selected.ok_or(
        "Carlton exposes no uniform-Right Story with at least three content-bearing canonical paragraphs",
    )?;
    let story = session
        .graph()
        .stories
        .get(&story_id)
        .ok_or("selected Story disappeared")?
        .clone();
    let paragraph_ids = paragraphs
        .iter()
        .map(|paragraph| paragraph.paragraph_id)
        .collect::<Vec<_>>();

    fs::create_dir_all(&output)?;

    session.set_paragraph_alignment_override_v1(
        paragraph_ids.clone(),
        AuthoredParagraphAlignmentValueV1::Center,
    )?;
    write_state(&session, &output, "center")?;

    session.clear_paragraph_alignment_override_v1(paragraph_ids.clone())?;
    write_state(&session, &output, "clear")?;

    session.set_paragraph_alignment_override_v1(
        vec![paragraph_ids[0]],
        AuthoredParagraphAlignmentValueV1::Left,
    )?;
    session.set_paragraph_alignment_override_v1(
        vec![paragraph_ids[1]],
        AuthoredParagraphAlignmentValueV1::Center,
    )?;
    write_state(&session, &output, "mixed")?;

    let paragraph_json = paragraphs
        .iter()
        .enumerate()
        .map(|(index, paragraph)| {
            let text = scalar_slice(&story.text, paragraph.range.start, paragraph.range.end)?;
            Ok::<Value, Box<dyn Error>>(json!({
                "ordinal": index,
                "paragraph_id": paragraph.paragraph_id.as_canonical().to_string(),
                "range": {
                    "start": paragraph.range.start,
                    "end": paragraph.range.end,
                },
                "non_whitespace_sha256": non_whitespace_sha256(&text),
                "non_whitespace_scalar_count": text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .count(),
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let center = vec!["center"; paragraphs.len()];
    let clear = vec!["right"; paragraphs.len()];
    let mut mixed = vec!["right"; paragraphs.len()];
    mixed[0] = "left";
    mixed[1] = "center";

    let expected = json!({
        "schema": SCHEMA,
        "source_sha256": hash.to_string(),
        "story_id": story_id.as_canonical().to_string(),
        "story_non_whitespace_sha256": non_whitespace_sha256(&story.text),
        "paragraph_count": paragraphs.len(),
        "paragraphs": paragraph_json,
        "states": {
            "center": {
                "alignment_sequence": center,
                "wire_contract": "scoped"
            },
            "clear": {
                "alignment_sequence": clear,
                "wire_contract": "full_story"
            },
            "mixed": {
                "alignment_sequence": mixed,
                "wire_contract": "scoped"
            }
        },
        "claims": {
            "source_story_text_recorded": false,
            "target_side_edit_proven": false,
            "capability_promoted": false
        }
    });
    fs::write(
        output.join("expected.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&expected)?);
    Ok(())
}
