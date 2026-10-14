//! Deterministic text-first DOCX target adapter.
//!
//! This adapter deliberately preserves only recovered editable Story text in
//! its first bounded profile. Publisher fixed-layout placement, linked flow,
//! graphics and richer authoring semantics stay in the ExportPlan/LossReport;
//! they are not inferred or hidden inside the DOCX writer.

use pub_export::ExportPlan;
use pub_model::{Story, StoryId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const DOCX_ADAPTER_VERSION_V0_1: &str = "docx-v0.1";
pub const DOCX_PROFILE_TEXT_FIRST: &str = "text-first-editable";
pub const DOCX_SCHEMA_FENCE_WORDPROCESSINGML_2006: &str = "wordprocessingml-2006-transitional";

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const ROOT_RELS_PATH: &str = "_rels/.rels";
const DOCUMENT_PATH: &str = "word/document.xml";

#[derive(Debug)]
pub enum DocxError {
    TargetFormatMismatch { actual: String },
    ExportBlocked,
    Zip(zip::result::ZipError),
    Io(std::io::Error),
    InvalidWrittenArchive(String),
}

impl fmt::Display for DocxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetFormatMismatch { actual } => {
                write!(formatter, "DOCX adapter received target format {actual}")
            }
            Self::ExportBlocked => formatter.write_str("DOCX export plan is blocked"),
            Self::Zip(error) => write!(formatter, "DOCX ZIP error: {error}"),
            Self::Io(error) => write!(formatter, "DOCX I/O error: {error}"),
            Self::InvalidWrittenArchive(message) => {
                write!(formatter, "written DOCX failed self-validation: {message}")
            }
        }
    }
}

impl std::error::Error for DocxError {}

impl From<zip::result::ZipError> for DocxError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Zip(value)
    }
}

impl From<std::io::Error> for DocxError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn write_text_first_docx(
    plan: &ExportPlan,
    stories: &BTreeMap<StoryId, Story>,
) -> Result<Vec<u8>, DocxError> {
    if plan.target.format != "docx" {
        return Err(DocxError::TargetFormatMismatch {
            actual: plan.target.format.clone(),
        });
    }
    if !plan.can_serialize() {
        return Err(DocxError::ExportBlocked);
    }

    let document = document_xml(stories);
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = deterministic_stored_options();

    writer.start_file(CONTENT_TYPES_PATH, options)?;
    writer.write_all(content_types_xml().as_bytes())?;
    writer.start_file(ROOT_RELS_PATH, options)?;
    writer.write_all(root_rels_xml().as_bytes())?;
    writer.start_file(DOCUMENT_PATH, options)?;
    writer.write_all(document.as_bytes())?;

    let bytes = writer.finish()?.into_inner();
    validate_written_docx(&bytes)?;
    Ok(bytes)
}

fn deterministic_stored_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
}

fn content_types_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>
"#
}

fn root_rels_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"#
}

fn document_xml(stories: &BTreeMap<StoryId, Story>) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
"#,
    );

    for story in stories.values().filter(|story| !story.text.is_empty()) {
        for paragraph in story_paragraphs(&story.text) {
            xml.push_str(r#"    <w:p><w:r><w:t xml:space="preserve">"#);
            xml.push_str(&escape_xml_text(paragraph));
            xml.push_str("</w:t></w:r></w:p>\n");
        }
    }

    xml.push_str("    <w:sectPr/>\n  </w:body>\n</w:document>\n");
    xml
}

fn story_paragraphs(text: &str) -> Vec<&str> {
    let mut paragraphs = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\r' || bytes[index] == b'\n' {
            paragraphs.push(&text[start..index]);
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            start = index + 1;
        }
        index += 1;
    }
    paragraphs.push(&text[start..]);
    paragraphs
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn validate_written_docx(bytes: &[u8]) -> Result<(), DocxError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let names = (0..archive.len())
        .map(|index| archive.by_index(index).map(|file| file.name().to_owned()))
        .collect::<Result<BTreeSet<_>, _>>()?;

    for required in [CONTENT_TYPES_PATH, ROOT_RELS_PATH, DOCUMENT_PATH] {
        if !names.contains(required) {
            return Err(DocxError::InvalidWrittenArchive(format!(
                "missing required part {required}"
            )));
        }
    }

    let mut document = String::new();
    archive
        .by_name(DOCUMENT_PATH)?
        .read_to_string(&mut document)?;
    if !document.contains("<w:document ")
        || !document.contains("<w:body>")
        || !document.contains("<w:sectPr/>")
    {
        return Err(DocxError::InvalidWrittenArchive(
            "word/document.xml lacks required WordprocessingML structure".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_export::{ExportPlan, TargetProfile};
    use pub_model::CanonicalId;

    fn story_id(byte: u8) -> StoryId {
        StoryId::from_canonical(CanonicalId::from_bytes([byte; 16]))
    }

    fn plan() -> ExportPlan {
        ExportPlan {
            schema_version: "0.1".into(),
            target: TargetProfile {
                format: "docx".into(),
                adapter_version: DOCX_ADAPTER_VERSION_V0_1.into(),
                profile: DOCX_PROFILE_TEXT_FIRST.into(),
                schema_fence: Some(DOCX_SCHEMA_FENCE_WORDPROCESSINGML_2006.into()),
            },
            conversion_fence: None,
            features: Vec::new(),
            losses: Vec::new(),
            blockers: Vec::new(),
        }
    }

    #[test]
    fn writes_deterministic_editable_story_text() {
        let mut stories = BTreeMap::new();
        stories.insert(
            story_id(1),
            Story {
                id: story_id(1),
                text: "Hello & <Word>\rSecond line".into(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            },
        );
        let first = write_text_first_docx(&plan(), &stories).unwrap();
        let second = write_text_first_docx(&plan(), &stories).unwrap();
        assert_eq!(first, second);

        let mut archive = ZipArchive::new(Cursor::new(first)).unwrap();
        let mut xml = String::new();
        archive
            .by_name(DOCUMENT_PATH)
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("Hello &amp; &lt;Word&gt;"));
        assert!(xml.contains("Second line"));
        assert_eq!(xml.matches("<w:p>").count(), 2);
    }
}
