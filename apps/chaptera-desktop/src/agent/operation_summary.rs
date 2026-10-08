use pub_editor::EditOperation;
use serde_json::{Value, json};

use super::{rect_json, sha256_hex};

pub(super) fn operation_summary(operation: &EditOperation) -> Value {
    match operation {
        EditOperation::LinkTextFrameTail { transition } => json!({
            "kind":"link_text_frame_tail",
            "story_id":transition.story_id.as_canonical().to_string(),
            "source_frame_id":transition.source_frame_id.as_canonical().to_string(),
            "target_frame_id":transition.target_frame_id.as_canonical().to_string(),
            "target_empty_story_id":transition.target_empty_story.id.as_canonical().to_string()
        }),
        EditOperation::ReplaceStoryRange {
            story_id,
            start_scalar,
            end_scalar,
            before_story_state_id,
            after_story_state_id,
            ..
        } => json!({
            "kind":"replace_story_range",
            "story_id":story_id.as_canonical().to_string(),
            "start_scalar":start_scalar,
            "end_scalar":end_scalar,
            "before_story_state_id":before_story_state_id,
            "after_story_state_id":after_story_state_id
        }),
        EditOperation::ReplaceStoryText {
            story_id,
            before,
            after,
        } => json!({
            "kind":"replace_story_text",
            "story_id":story_id.as_canonical().to_string(),
            "before_text_sha256":sha256_hex(before.as_bytes()),
            "after_text_sha256":sha256_hex(after.as_bytes())
        }),
        EditOperation::BreakTextFrameForwardLink {
            story_id,
            upstream_frame_id,
            downstream_frame_id,
            new_story_id,
            ..
        } => json!({
            "kind":"break_text_frame_forward_link",
            "story_id":story_id.as_canonical().to_string(),
            "upstream_frame_id":upstream_frame_id.as_canonical().to_string(),
            "downstream_frame_id":downstream_frame_id.as_canonical().to_string(),
            "new_story_id":new_story_id.as_canonical().to_string()
        }),
        EditOperation::ReplaceTableCellText {
            node_id,
            story_id,
            cell_id,
            ..
        } => json!({
            "kind":"replace_table_cell_text",
            "node_id":node_id.as_canonical().to_string(),
            "story_id":story_id.as_canonical().to_string(),
            "cell_id":cell_id.as_canonical().to_string()
        }),
        EditOperation::ReplaceImage {
            node_id,
            before_asset,
            after_asset,
        } => json!({
            "kind":"replace_image",
            "node_id":node_id.as_canonical().to_string(),
            "before_asset":before_asset.as_ref().map(|value| value.to_string()),
            "after_asset":after_asset.to_string()
        }),
        EditOperation::SetImageCrop {
            node_id,
            before,
            after,
        } => json!({
            "kind":"set_image_crop",
            "node_id":node_id.as_canonical().to_string(),
            "before_crop_state_sha256":sha256_hex(
                &serde_json::to_vec(before)
                    .expect("ImageCropStateV1 JSON serialization is infallible")
            ),
            "after_crop_state_sha256":sha256_hex(
                &serde_json::to_vec(after)
                    .expect("ImageCropStateV1 JSON serialization is infallible")
            )
        }),
        EditOperation::MoveNode {
            node_id,
            before,
            after,
        } => json!({
            "kind":"move_node",
            "node_id":node_id.as_canonical().to_string(),
            "before":rect_json(*before),
            "after":rect_json(*after)
        }),
        EditOperation::MoveNodes { page_id, entries } => json!({
            "kind":"move_nodes",
            "page_id":page_id.as_canonical().to_string(),
            "entries":entries.iter().map(|entry| json!({
                "node_id":entry.node_id.as_canonical().to_string(),
                "before":rect_json(entry.before),
                "after":rect_json(entry.after)
            })).collect::<Vec<_>>()
        }),
        EditOperation::ResizeNode {
            node_id,
            before,
            after,
        } => json!({
            "kind":"resize_node",
            "node_id":node_id.as_canonical().to_string(),
            "before":rect_json(*before),
            "after":rect_json(*after)
        }),
        EditOperation::ResizeNodes { page_id, entries } => json!({
            "kind":"resize_nodes",
            "page_id":page_id.as_canonical().to_string(),
            "entries":entries.iter().map(|entry| json!({
                "node_id":entry.node_id.as_canonical().to_string(),
                "before":rect_json(entry.before),
                "after":rect_json(entry.after)
            })).collect::<Vec<_>>()
        }),
        EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            text_preset,
        } => json!({
            "kind":"create_text_box",
            "node_id":node_id.as_canonical().to_string(),
            "story_id":story_id.as_canonical().to_string(),
            "page_id":page_id.as_canonical().to_string(),
            "bounds":rect_json(*bounds),
            "text_preset":text_preset
        }),
        EditOperation::CreateShape {
            node_id,
            page_id,
            parent_id,
            shape_kind,
            bounds,
            transform,
            paint,
            provenance,
        } => json!({
            "kind":"create_shape",
            "node_id":node_id.as_canonical().to_string(),
            "page_id":page_id.as_canonical().to_string(),
            "parent_id":parent_id.as_canonical().to_string(),
            "shape_kind":shape_kind,
            "bounds":rect_json(*bounds),
            "transform":transform,
            "paint":paint,
            "provenance":provenance
        }),
        EditOperation::CreateLine {
            node_id,
            page_id,
            parent_id,
            geometry,
            stroke,
            provenance,
        } => json!({
            "kind":"create_line",
            "node_id":node_id.as_canonical().to_string(),
            "page_id":page_id.as_canonical().to_string(),
            "parent_id":parent_id.as_canonical().to_string(),
            "geometry":geometry,
            "stroke":stroke,
            "provenance":provenance
        }),
        EditOperation::CreateTable { table } => json!({
            "kind":"create_table",
            "node_id":table.node_id.as_canonical().to_string(),
            "story_id":table.story_id.as_canonical().to_string(),
            "page_id":table.page_id.as_canonical().to_string(),
            "bounds":rect_json(table.bounds),
            "row_ids":table
                .row_ids
                .iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "column_ids":table
                .column_ids
                .iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "cell_ids":table
                .cell_ids
                .iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>()
        }),
        EditOperation::SetTableTrackExtent { history } => json!({
            "kind":"set_table_track_extent",
            "table_id":history.table_id.as_canonical().to_string(),
            "target":history.target,
            "before_extent":history.before_extent,
            "after_extent":history.after_extent,
            "before_bounds":rect_json(history.before_bounds),
            "after_bounds":rect_json(history.after_bounds)
        }),
        EditOperation::InsertTableRow { history } => json!({
            "kind":"insert_table_row",
            "table_id":history.table_id.as_canonical().to_string(),
            "story_id":history.story_id.as_canonical().to_string(),
            "mutation":history.mutation,
            "before_bounds":rect_json(history.before.bounds),
            "after_bounds":rect_json(history.after.bounds)
        }),
        EditOperation::DeleteTableRow { history } => json!({
            "kind":"delete_table_row",
            "table_id":history.table_id.as_canonical().to_string(),
            "story_id":history.story_id.as_canonical().to_string(),
            "mutation":history.mutation,
            "before_bounds":rect_json(history.before.bounds),
            "after_bounds":rect_json(history.after.bounds)
        }),
        EditOperation::InsertTableColumn { history } => json!({
            "kind":"insert_table_column",
            "table_id":history.table_id.as_canonical().to_string(),
            "story_id":history.story_id.as_canonical().to_string(),
            "mutation":history.mutation,
            "before_bounds":rect_json(history.before.bounds),
            "after_bounds":rect_json(history.after.bounds)
        }),
        EditOperation::DeleteTableColumn { history } => json!({
            "kind":"delete_table_column",
            "table_id":history.table_id.as_canonical().to_string(),
            "story_id":history.story_id.as_canonical().to_string(),
            "mutation":history.mutation,
            "before_bounds":rect_json(history.before.bounds),
            "after_bounds":rect_json(history.after.bounds)
        }),
        EditOperation::DeleteNode {
            node_id,
            page_id,
            before_state_id,
            ..
        } => json!({
            "kind":"delete_node",
            "node_id":node_id.as_canonical().to_string(),
            "page_id":page_id.as_canonical().to_string(),
            "before_state_id":before_state_id
        }),
        EditOperation::ReorderAuthoredStack { transition } => json!({
            "kind":"reorder_authored_stack",
            "node_id":transition.node_id.as_canonical().to_string(),
            "page_id":transition.page_id.as_canonical().to_string(),
            "mode":transition.mode,
            "before_index":transition.before_index,
            "after_index":transition.after_index,
            "before_state_id":transition.before_state_id,
            "after_state_id":transition.after_state_id
        }),
        EditOperation::ReorderPagesV1 { transition } => json!({
            "kind": "reorder_pages_v1",
            "document_id": transition.document_id.as_canonical().to_string(),
            "before_page_ids": transition.before.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "after_page_ids": transition.after.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "before_state_id": transition.before_state_id.as_str(),
            "after_state_id": transition.after_state_id.as_str(),
        }),
        EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            after_state_hash,
        } => json!({
            "kind":"set_text_format_property",
            "story_id":story_id.as_canonical().to_string(),
            "start_scalar":start_scalar,
            "end_scalar":end_scalar,
            "property":property,
            "value":value,
            "before_state_hash":before_state_hash,
            "after_state_hash":after_state_hash
        }),
        EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            after_state_hash,
        } => json!({
            "kind":"clear_text_format_property_override",
            "story_id":story_id.as_canonical().to_string(),
            "start_scalar":start_scalar,
            "end_scalar":end_scalar,
            "property":property,
            "before_state_hash":before_state_hash,
            "after_state_hash":after_state_hash
        }),
        EditOperation::SetTextFormatPropertyScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            after_state_hash,
        } => json!({
            "kind":"set_text_format_property_scoped_v1",
            "story_id":story_id.as_canonical().to_string(),
            "start_scalar":start_scalar,
            "end_scalar":end_scalar,
            "property":property,
            "value":value,
            "before_state_hash":before_state_hash,
            "after_state_hash":after_state_hash
        }),
        EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            after_state_hash,
        } => json!({
            "kind":"clear_text_format_property_override_scoped_v1",
            "story_id":story_id.as_canonical().to_string(),
            "start_scalar":start_scalar,
            "end_scalar":end_scalar,
            "property":property,
            "before_state_hash":before_state_hash,
            "after_state_hash":after_state_hash
        }),
        EditOperation::SetParagraphAlignmentOverride {
            paragraph_ids,
            value,
            before,
            after,
        } => json!({
            "kind":"set_paragraph_alignment_override",
            "paragraph_ids":paragraph_ids
                .iter()
                .map(|paragraph_id| paragraph_id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "value":value,
            "before":before,
            "after":after
        }),
        EditOperation::ClearParagraphAlignmentOverride {
            paragraph_ids,
            before,
            after,
        } => json!({
            "kind":"clear_paragraph_alignment_override",
            "paragraph_ids":paragraph_ids
                .iter()
                .map(|paragraph_id| paragraph_id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "before":before,
            "after":after
        }),
    }
}
