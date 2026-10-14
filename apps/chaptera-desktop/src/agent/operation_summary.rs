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
        EditOperation::RegisterAuthoredPageIdentityV1 { identity } => json!({
            "kind": "register_authored_page_identity_v1",
            "page_id": identity.page_id.as_canonical().to_string(),
            "provenance": identity.provenance,
        }),
        EditOperation::InsertBlankPageAfterV1 { transition } => json!({
            "kind": "insert_blank_page_after_v1",
            "document_id": transition.document_id.as_canonical().to_string(),
            "anchor_page_id": transition.anchor_page_id.as_canonical().to_string(),
            "destination_page_id": transition.identity.page_id.as_canonical().to_string(),
            "insertion_index": transition.insertion_index,
            "before_customer_page_ids": transition.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "after_customer_page_ids": transition.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "before_document_state_id": transition.before_document_state_id.as_str(),
            "after_document_state_id": transition.after_document_state_id.as_str(),
        }),
        EditOperation::DuplicateBlankPageV1 { transition } => json!({
            "kind": "duplicate_blank_page_v1",
            "document_id": transition.document_id.as_canonical().to_string(),
            "source_page_id": transition.source_page_id.as_canonical().to_string(),
            "destination_page_id": transition.destination_identity.page_id.as_canonical().to_string(),
            "before_customer_page_ids": transition.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "after_customer_page_ids": transition.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "insertion_index": transition.insertion_index,
            "before_document_state_id": transition.before_document_state_id.as_str(),
            "after_document_state_id": transition.after_document_state_id.as_str(),
        }),
        EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => json!({
            "kind": "duplicate_authored_rectangles_page_v1",
            "document_id": transition.page.document_id.as_canonical().to_string(),
            "source_page_id": transition.page.source_page_id.as_canonical().to_string(),
            "destination_page_id": transition.page.destination_identity.page_id.as_canonical().to_string(),
            "source_rectangle_node_ids": transition.source_shapes.iter()
                .map(|shape| shape.node_id.as_canonical().to_string()).collect::<Vec<_>>(),
            "destination_rectangle_node_ids": transition.destination_shapes.iter()
                .map(|shape| shape.node_id.as_canonical().to_string()).collect::<Vec<_>>(),
            "insertion_index": transition.page.insertion_index,
            "before_customer_page_ids": transition.page.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "after_customer_page_ids": transition.page.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "before_state_id": transition.before_state_id.as_str(),
            "after_state_id": transition.after_state_id.as_str(),
        }),
        EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => json!({
            "kind": "duplicate_authored_rectangle_page_v1",
            "document_id": transition.page.document_id.as_canonical().to_string(),
            "source_page_id": transition.page.source_page_id.as_canonical().to_string(),
            "destination_page_id": transition.page.destination_identity.page_id.as_canonical().to_string(),
            "source_rectangle_node_id": transition.source_shape.node_id.as_canonical().to_string(),
            "destination_rectangle_node_id": transition.destination_shape.node_id.as_canonical().to_string(),
            "insertion_index": transition.page.insertion_index,
            "before_customer_page_ids": transition.page.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "after_customer_page_ids": transition.page.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "before_state_id": transition.before_state_id.as_str(),
            "after_state_id": transition.after_state_id.as_str(),
        }),
        EditOperation::DeleteAuthoredRectanglesPageV1 { transition } => json!({
            "kind": "delete_authored_rectangles_page_v1",
            "document_id": transition.page.document_id.as_canonical().to_string(),
            "page_id": transition.page.identity.page_id.as_canonical().to_string(),
            "rectangle_node_ids": transition.shapes_before.iter()
                .map(|shape| shape.node_id.as_canonical().to_string()).collect::<Vec<_>>(),
            "removal_index": transition.page.removal_index,
            "before_customer_page_ids": transition.page.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "after_customer_page_ids": transition.page.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "before_state_id": transition.before_state_id.as_str(),
            "after_state_id": transition.after_state_id.as_str(),
        }),
        EditOperation::DeleteAuthoredRectanglePageV1 { transition } => json!({
            "kind": "delete_authored_rectangle_page_v1",
            "document_id": transition.page.document_id.as_canonical().to_string(),
            "page_id": transition.page.identity.page_id.as_canonical().to_string(),
            "rectangle_node_id": transition.shape_before.node_id.as_canonical().to_string(),
            "removal_index": transition.page.removal_index,
            "before_customer_page_ids": transition.page.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "after_customer_page_ids": transition.page.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string()).collect::<Vec<_>>(),
            "before_state_id": transition.before_state_id.as_str(),
            "after_state_id": transition.after_state_id.as_str(),
        }),
        EditOperation::DeleteBlankAuthoredPageV1 { transition } => json!({
            "kind": "delete_blank_authored_page_v1",
            "document_id": transition.document_id.as_canonical().to_string(),
            "page_id": transition.identity.page_id.as_canonical().to_string(),
            "before_customer_page_ids": transition.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "after_customer_page_ids": transition.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "removal_index": transition.removal_index,
            "before_document_state_id": transition.before_document_state_id.as_str(),
            "after_document_state_id": transition.after_document_state_id.as_str(),
        }),
        EditOperation::AppendBlankPageV1 { transition } => json!({
            "kind": "append_blank_page_v1",
            "document_id": transition.document_id.as_canonical().to_string(),
            "page_id": transition.identity.page_id.as_canonical().to_string(),
            "provenance": transition.identity.provenance,
            "width_emu": transition.page.size.width.get(),
            "height_emu": transition.page.size.height.get(),
            "before_customer_page_ids": transition.before_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "after_customer_page_ids": transition.after_customer_page_ids.iter()
                .map(|id| id.as_canonical().to_string())
                .collect::<Vec<_>>(),
            "before_document_state_id": transition.before_document_state_id.as_str(),
            "after_document_state_id": transition.after_document_state_id.as_str(),
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
