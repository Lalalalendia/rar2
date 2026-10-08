use pub_editor::PageOrderTransitionV1;
use serde_json::{Value, json};

pub(super) fn operation_summary(transition: &PageOrderTransitionV1) -> Value {
    json!({
        "kind": "reorder_pages_v1",
        "document_id": transition.document_id.as_canonical().to_string(),
        "before_page_ids": transition.before.iter()
            .map(|id| id.as_canonical().to_string())
            .collect::<Vec<_>>(),
        "after_page_ids": transition.after.iter()
            .map(|id| id.as_canonical().to_string())
            .collect::<Vec<_>>(),
        "before_state_id": transition.before_state_id,
        "after_state_id": transition.after_state_id,
    })
}
