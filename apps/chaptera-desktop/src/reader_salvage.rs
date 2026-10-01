use pub_viewer::{
    ReaderPartialSourceFact, ReaderPartialSourceGap, ReaderPartialSourceGraph,
    ReaderSalvageStreamState,
};

pub const MAX_SEARCH_RESULTS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalvageTextMatch {
    pub story_key: String,
    pub utf16_start: u32,
    pub utf16_end: u32,
    pub text: String,
}

pub fn has_recovered_text(graph: &ReaderPartialSourceGraph) -> bool {
    graph.facts.iter().any(|fact| {
        matches!(
            fact,
            ReaderPartialSourceFact::TextRange { text, .. } if !text.is_empty()
        )
    })
}

pub fn recovered_text(graph: &ReaderPartialSourceGraph) -> String {
    graph
        .facts
        .iter()
        .filter_map(|fact| match fact {
            ReaderPartialSourceFact::TextRange { text, .. } if !text.is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn search_text(graph: &ReaderPartialSourceGraph, query: &str) -> Vec<SalvageTextMatch> {
    if query.is_empty() || query.chars().count() > 200 {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for fact in &graph.facts {
        let ReaderPartialSourceFact::TextRange {
            story_key,
            utf16_start,
            text,
            ..
        } = fact
        else {
            continue;
        };

        for (byte_start, matched) in text.match_indices(query) {
            let relative_utf16_start = text[..byte_start].encode_utf16().count();
            let relative_utf16_end =
                relative_utf16_start.saturating_add(matched.encode_utf16().count());
            let Ok(relative_utf16_start) = u32::try_from(relative_utf16_start) else {
                continue;
            };
            let Ok(relative_utf16_end) = u32::try_from(relative_utf16_end) else {
                continue;
            };
            let Some(match_start) = utf16_start.checked_add(relative_utf16_start) else {
                continue;
            };
            let Some(match_end) = utf16_start.checked_add(relative_utf16_end) else {
                continue;
            };

            matches.push(SalvageTextMatch {
                story_key: story_key.clone(),
                utf16_start: match_start,
                utf16_end: match_end,
                text: matched.to_owned(),
            });
            if matches.len() == MAX_SEARCH_RESULTS {
                return matches;
            }
        }
    }
    matches
}

pub fn subsystem_rows(
    graph: &ReaderPartialSourceGraph,
) -> [(&'static str, &'static str); 4] {
    [
        ("Contents", stream_state_label(graph.subsystems.contents)),
        ("Text", stream_state_label(graph.subsystems.quill)),
        ("Graphics", stream_state_label(graph.subsystems.escher)),
        (
            "Delayed images",
            stream_state_label(graph.subsystems.escher_delay),
        ),
    ]
}

pub fn gap_labels(graph: &ReaderPartialSourceGraph) -> Vec<&'static str> {
    graph
        .gaps
        .iter()
        .map(|gap| match gap {
            ReaderPartialSourceGap::TextUnavailable => "Recovered text is unavailable.",
            ReaderPartialSourceGap::TextSemanticAmbiguity => {
                "Some recovered text could not be admitted safely."
            }
            ReaderPartialSourceGap::ImageFactsUnavailable => {
                "No independently verified image facts are available."
            }
            ReaderPartialSourceGap::GeometryFactsUnavailable => {
                "Page/object geometry is unavailable; no page layout is claimed."
            }
        })
        .collect()
}

fn stream_state_label(state: ReaderSalvageStreamState) -> &'static str {
    match state {
        ReaderSalvageStreamState::NotAttempted => "not attempted",
        ReaderSalvageStreamState::Readable => "readable",
        ReaderSalvageStreamState::RecoveredRootRegular => "recovered",
        ReaderSalvageStreamState::Absent => "absent",
        ReaderSalvageStreamState::PresentOverLimit => "over limit",
        ReaderSalvageStreamState::PresentUnreadable => "unreadable",
        ReaderSalvageStreamState::ContainerUnavailable => "container unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_viewer::{READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1, ReaderSalvageSubsystemProbe};

    fn graph() -> ReaderPartialSourceGraph {
        ReaderPartialSourceGraph {
            schema_version: READER_PARTIAL_SOURCE_GRAPH_SCHEMA_V1.to_owned(),
            source_sha256: "a".repeat(64),
            contents_family: Some("0x2c".to_owned()),
            subsystems: ReaderSalvageSubsystemProbe {
                contents: ReaderSalvageStreamState::Readable,
                quill: ReaderSalvageStreamState::Readable,
                escher: ReaderSalvageStreamState::Absent,
                escher_delay: ReaderSalvageStreamState::Absent,
            },
            facts: vec![ReaderPartialSourceFact::TextRange {
                story_key: "quill-syid:00000001".to_owned(),
                utf16_start: 10,
                utf16_end: 22,
                text: "Hello 😀 PUB".to_owned(),
            }],
            gaps: vec![
                ReaderPartialSourceGap::ImageFactsUnavailable,
                ReaderPartialSourceGap::GeometryFactsUnavailable,
            ],
        }
    }

    #[test]
    fn search_preserves_utf16_offsets_without_inventing_page_placement() {
        let graph = graph();
        let matches = search_text(&graph, "PUB");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].story_key, "quill-syid:00000001");
        assert_eq!(matches[0].utf16_start, 19);
        assert_eq!(matches[0].utf16_end, 22);
    }

    #[test]
    fn recovered_text_is_copyable_but_geometry_gap_stays_explicit() {
        let graph = graph();
        assert!(has_recovered_text(&graph));
        assert_eq!(recovered_text(&graph), "Hello 😀 PUB");
        assert!(
            gap_labels(&graph)
                .iter()
                .any(|label| label.contains("no page layout is claimed"))
        );
    }
}
