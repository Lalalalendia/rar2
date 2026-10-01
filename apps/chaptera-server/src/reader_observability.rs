use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::http::HeaderMap;
use serde::Serialize;

pub const TRACE_PROTOCOL_VERSION: &str = "chaptera.trace-context.v1";
const MAX_RECENT_EVENTS: usize = 512;
const MAX_SESSION_CONTEXTS: usize = 4096;

const TRACE_VERSION_HEADER: &str = "x-chaptera-trace-version";
const TRACE_ID_HEADER: &str = "x-chaptera-trace-id";
const INTERACTION_ID_HEADER: &str = "x-chaptera-interaction-id";
const SESSION_INCARNATION_HEADER: &str = "x-chaptera-session-incarnation";
const OPERATION_CLASS_HEADER: &str = "x-chaptera-operation-class";
const BROWSER_FAMILY_HEADER: &str = "x-chaptera-browser-family";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderTraceContextV1 {
    pub protocol_version: String,
    pub trace_id: String,
    pub interaction_id: String,
    pub session_incarnation: String,
    pub operation_class: String,
    pub browser_family: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReaderMetricLabelsV1 {
    pub stage: &'static str,
    pub operation_class: String,
    pub outcome: &'static str,
    pub region: &'static str,
    pub protocol_major: &'static str,
    pub browser_family: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReaderTraceEventV1 {
    pub protocol_version: &'static str,
    pub metric_labels: ReaderMetricLabelsV1,
    pub trace_id: String,
    pub interaction_id: String,
    pub session_incarnation: String,
    pub duration_ms: f64,
}

#[derive(Default)]
struct ReaderObservabilityInner {
    recent_events: VecDeque<ReaderTraceEventV1>,
    session_contexts: HashMap<String, ReaderTraceContextV1>,
}

#[derive(Clone, Default)]
pub struct ReaderObservabilityV1 {
    inner: Arc<Mutex<ReaderObservabilityInner>>,
}

impl ReaderObservabilityV1 {
    pub fn context_from_headers(headers: &HeaderMap) -> Option<ReaderTraceContextV1> {
        let value = |name: &str| headers.get(name)?.to_str().ok().map(str::to_owned);
        let protocol_version = value(TRACE_VERSION_HEADER)?;
        let trace_id = value(TRACE_ID_HEADER)?;
        let interaction_id = value(INTERACTION_ID_HEADER)?;
        let session_incarnation = value(SESSION_INCARNATION_HEADER)?;
        let operation_class = value(OPERATION_CLASS_HEADER)?;
        let browser_family = value(BROWSER_FAMILY_HEADER)?;

        if protocol_version != TRACE_PROTOCOL_VERSION
            || !bounded_id(&trace_id)
            || !bounded_id(&interaction_id)
            || !bounded_id(&session_incarnation)
            || !matches!(
                operation_class.as_str(),
                "commit" | "scene_read" | "open" | "reconnect" | "export" | "asset" | "other"
            )
            || !matches!(
                browser_family.as_str(),
                "chromium" | "firefox" | "webkit" | "other" | "unknown"
            )
        {
            return None;
        }

        Some(ReaderTraceContextV1 {
            protocol_version,
            trace_id,
            interaction_id,
            session_incarnation,
            operation_class,
            browser_family,
        })
    }

    pub fn remember_session(&self, session_id: &str, context: Option<&ReaderTraceContextV1>) {
        let Some(context) = context else {
            return;
        };
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.session_contexts.len() >= MAX_SESSION_CONTEXTS
            && !inner.session_contexts.contains_key(session_id)
            && let Some(oldest) = inner.session_contexts.keys().next().cloned()
        {
            inner.session_contexts.remove(&oldest);
        }
        inner
            .session_contexts
            .insert(session_id.to_owned(), context.clone());
    }

    pub fn session_context(&self, session_id: &str) -> Option<ReaderTraceContextV1> {
        self.inner
            .lock()
            .ok()?
            .session_contexts
            .get(session_id)
            .cloned()
    }

    pub fn forget_session(&self, session_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.session_contexts.remove(session_id);
        }
    }

    pub fn record(
        &self,
        stage: &'static str,
        context: Option<&ReaderTraceContextV1>,
        outcome: &'static str,
        duration: Duration,
    ) {
        let Some(context) = context else {
            return;
        };
        if !matches!(
            stage,
            "reader.session_create"
                | "reader.upload"
                | "reader.open"
                | "reader.scan"
                | "reader.structural_scan"
                | "reader.scene"
                | "reader.cleanup"
        ) || !matches!(outcome, "success" | "error" | "rejected" | "unknown")
        {
            return;
        }

        let event = ReaderTraceEventV1 {
            protocol_version: TRACE_PROTOCOL_VERSION,
            metric_labels: ReaderMetricLabelsV1 {
                stage,
                operation_class: context.operation_class.clone(),
                outcome,
                region: "unknown",
                protocol_major: "v1",
                browser_family: context.browser_family.clone(),
            },
            trace_id: context.trace_id.clone(),
            interaction_id: context.interaction_id.clone(),
            session_incarnation: context.session_incarnation.clone(),
            duration_ms: (duration.as_secs_f64() * 1_000_000.0).round() / 1_000.0,
        };

        if let Ok(encoded) = serde_json::to_string(&event) {
            eprintln!("chaptera_reader_trace {encoded}");
        }

        if let Ok(mut inner) = self.inner.lock() {
            inner.recent_events.push_back(event);
            while inner.recent_events.len() > MAX_RECENT_EVENTS {
                inner.recent_events.pop_front();
            }
        }
    }

    #[cfg(test)]
    fn receipt(&self, trace_id: &str) -> Vec<ReaderTraceEventV1> {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .recent_events
                    .iter()
                    .filter(|event| event.trace_id == trace_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn bounded_id(value: &str) -> bool {
    (8..=160).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::*;

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACE_VERSION_HEADER,
            HeaderValue::from_static(TRACE_PROTOCOL_VERSION),
        );
        headers.insert(TRACE_ID_HEADER, HeaderValue::from_static("trace:12345678"));
        headers.insert(
            INTERACTION_ID_HEADER,
            HeaderValue::from_static("interaction:12345678"),
        );
        headers.insert(
            SESSION_INCARNATION_HEADER,
            HeaderValue::from_static("session:12345678"),
        );
        headers.insert(OPERATION_CLASS_HEADER, HeaderValue::from_static("open"));
        headers.insert(BROWSER_FAMILY_HEADER, HeaderValue::from_static("chromium"));
        headers
    }

    #[test]
    fn parses_existing_trace_contract_without_document_identity() {
        let context = ReaderObservabilityV1::context_from_headers(&headers()).unwrap();
        assert_eq!(context.protocol_version, TRACE_PROTOCOL_VERSION);
        assert_eq!(context.operation_class, "open");
        assert_eq!(context.browser_family, "chromium");
    }

    #[test]
    fn receipt_uses_only_bounded_metric_labels() {
        let recorder = ReaderObservabilityV1::default();
        let context = ReaderObservabilityV1::context_from_headers(&headers()).unwrap();
        recorder.record(
            "reader.open",
            Some(&context),
            "success",
            Duration::from_millis(12),
        );
        let receipt = recorder.receipt(&context.trace_id);
        assert_eq!(receipt.len(), 1);
        let value = serde_json::to_value(&receipt[0]).unwrap();
        let labels = value["metric_labels"].as_object().unwrap();
        let mut keys = labels.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "browser_family",
                "operation_class",
                "outcome",
                "protocol_major",
                "region",
                "stage"
            ]
        );
        for prohibited in [
            "session_id",
            "upload_id",
            "source_sha256",
            "filename",
            "document_id",
            "story_id",
            "storage_locator",
            "token",
        ] {
            assert!(!labels.contains_key(prohibited));
        }
    }

    #[test]
    fn malformed_trace_headers_are_non_authoritative() {
        let mut invalid = headers();
        invalid.insert(TRACE_ID_HEADER, HeaderValue::from_static("tiny"));
        assert!(ReaderObservabilityV1::context_from_headers(&invalid).is_none());
    }
}
