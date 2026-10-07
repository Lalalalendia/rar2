//! Table-specific EditorSession orchestration.
//!
//! Pure table-track laws remain in pub-editor-table-core. This module owns the
//! session adapters that bind those laws and row/column history to the current
//! Editor graph/history without widening the root facade.

use super::*;

impl EditorSession {
    pub fn editable_table_cells_for_story(
        &self,
        story_id: StoryId,
    ) -> Vec<EditorEditableTableCell> {
        let mut result = Vec::new();

        for (node_id, node) in &self.graph.nodes {
            let Some(table) = node.payload.table.as_ref() else {
                continue;
            };
            if table.story_id != Some(story_id) {
                continue;
            }
            let Some(story) = self.graph.stories.get(&story_id) else {
                continue;
            };
            let Ok(cells) = materialize_bounded_simple_table_cells(table, story) else {
                continue;
            };

            for cell in cells {
                if self.can_replace_table_cell_text(*node_id, cell.id).is_err() {
                    continue;
                }
                result.push(EditorEditableTableCell {
                    node_id: *node_id,
                    story_id,
                    cell_id: cell.id,
                    row: cell.address.row,
                    column: cell.address.column,
                    text: cell.text,
                });
            }
        }

        result.sort_by_key(|cell| (cell.row, cell.column, cell.cell_id));
        result
    }

    pub fn can_replace_table_cell_text(
        &self,
        node_id: NodeId,
        cell_id: TableCellId,
    ) -> Result<(), EditorError> {
        self.validate_source_identity()?;

        let node = self
            .graph
            .nodes
            .get(&node_id)
            .ok_or(EditorError::TableEditUnsupported { node_id })?;
        let table = node
            .payload
            .table
            .as_ref()
            .ok_or(EditorError::TableEditUnsupported { node_id })?;
        let story_id = table
            .story_id
            .ok_or(EditorError::TableEditUnsupported { node_id })?;
        if self.story_has_paragraph_alignment_history_v1(story_id)? {
            return Err(EditorError::ParagraphAlignmentLifecycleUnsupported { story_id });
        }
        let story = self
            .graph
            .stories
            .get(&story_id)
            .ok_or(EditorError::TableEditUnsupported { node_id })?;

        let simple = table
            .simple_table
            .as_ref()
            .ok_or(EditorError::TableEditUnsupported { node_id })?;
        if !simple.cells_are_row_major()
            || !story.paragraphs.is_empty()
            || !story.runs.is_empty()
            || !story.fields.is_empty()
            || !story.hyperlinks.is_empty()
        {
            return Err(EditorError::TableEditUnsupported { node_id });
        }
        let table_owner_count = self
            .graph
            .nodes
            .values()
            .filter(|candidate| {
                candidate
                    .payload
                    .table_story
                    .as_ref()
                    .is_some_and(|owner| owner.story_id == Some(story_id))
                    || candidate
                        .payload
                        .table
                        .as_ref()
                        .is_some_and(|candidate_table| candidate_table.story_id == Some(story_id))
            })
            .count();
        if table_owner_count != 1 {
            return Err(EditorError::TableEditUnsupported { node_id });
        }

        let cells = materialize_bounded_simple_table_cells(table, story)
            .map_err(|_| EditorError::TableEditUnsupported { node_id })?;
        if !cells.iter().any(|cell| cell.id == cell_id) {
            return Err(EditorError::MissingTableCell { node_id, cell_id });
        }

        let (roundtrip_story, roundtrip_ranges) = rebuild_simple_table_story(table, &cells)
            .map_err(|_| EditorError::TableEditUnsupported { node_id })?;
        if roundtrip_story != story.text || roundtrip_ranges != snapshot_table_ranges(table) {
            return Err(EditorError::TableEditUnsupported { node_id });
        }

        Ok(())
    }

    pub fn replace_table_cell_text(
        &mut self,
        node_id: NodeId,
        cell_id: TableCellId,
        replacement: impl Into<String>,
    ) -> Result<EditOperation, EditorError> {
        self.can_replace_table_cell_text(node_id, cell_id)?;

        let replacement = replacement.into();
        let node = self
            .graph
            .nodes
            .get(&node_id)
            .expect("capability check verified table node");
        let table = node
            .payload
            .table
            .as_ref()
            .expect("capability check verified table payload");
        let story_id = table
            .story_id
            .expect("capability check verified table Story");
        let story = self
            .graph
            .stories
            .get(&story_id)
            .expect("capability check verified table Story presence");
        let mut cells = materialize_bounded_simple_table_cells(table, story)
            .expect("capability check verified table cell materialization");

        let target = cells
            .iter_mut()
            .find(|cell| cell.id == cell_id)
            .expect("capability check verified target cell");
        if target.text == replacement {
            return Err(EditorError::TableCellNoChange { node_id, cell_id });
        }
        target.text = replacement;

        let before_story = story.text.clone();
        let before_ranges = snapshot_table_ranges(table);
        let (after_story, after_ranges) = rebuild_simple_table_story(table, &cells)
            .map_err(|_| EditorError::TableEditUnsupported { node_id })?;

        let operation = EditOperation::ReplaceTableCellText {
            node_id,
            story_id,
            cell_id,
            before_story,
            after_story,
            before_ranges,
            after_ranges,
        };

        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn current_table_grid_v1(&self, table_id: NodeId) -> Option<EffectiveTableGridV1> {
        effective_table_grids_with_history(&self.graph, &self.undo)
            .into_iter()
            .find(|grid| grid.table_id == table_id)
    }

    pub fn current_table_bounds_v1(&self, table_id: NodeId) -> Option<RectEmu> {
        effective_table_bounds_with_history(&self.graph, &self.undo, table_id)
    }

    pub fn set_table_track_extent_v1(
        &mut self,
        table_id: NodeId,
        target: TableTrackTargetV1,
        after_extent: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.has_node_resize_history_v1(table_id) {
            return Err(EditorError::TableTrackResizeUnsupported { node_id: table_id });
        }
        let grid = self
            .current_table_grid_v1(table_id)
            .ok_or(EditorError::TableTrackResizeUnsupported { node_id: table_id })?;
        let bounds = self
            .current_table_bounds_v1(table_id)
            .ok_or(EditorError::TableTrackResizeUnsupported { node_id: table_id })?;
        let history = canonical_table_track_extent_history_v1(&grid, bounds, target, after_extent)
            .map_err(|_| EditorError::TableTrackResizeUnsupported { node_id: table_id })?;
        let operation = EditOperation::SetTableTrackExtent { history };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub(super) fn table_story_has_unremapped_range_metadata_v1(&self, story_id: StoryId) -> bool {
        self.source_typography_runs
            .iter()
            .any(|run| run.story_id == story_id)
            || self
                .source_typography_size_runs
                .iter()
                .any(|run| run.story_id == story_id)
            || self
                .source_paragraph_alignments
                .iter()
                .any(|run| run.story_id == story_id)
            || self
                .source_paragraph_flow_runs
                .iter()
                .any(|run| run.story_id == story_id)
            || self
                .undo
                .iter()
                .any(|operation| text_format_operation_story_id_v1(operation) == Some(story_id))
    }

    fn validate_table_rowcol_story_metadata_v1(
        &self,
        table_id: NodeId,
        story_id: StoryId,
    ) -> Result<(), EditorError> {
        let current_story_id = self
            .graph
            .nodes
            .get(&table_id)
            .and_then(|node| node.payload.table.as_ref())
            .and_then(|table| table.story_id)
            .ok_or(EditorError::TableRowColUnsupported { node_id: table_id })?;
        if current_story_id != story_id
            || self.table_story_has_unremapped_range_metadata_v1(story_id)
            || self.story_has_paragraph_alignment_history_v1(story_id)?
        {
            return Err(EditorError::TableRowColUnsupported { node_id: table_id });
        }
        Ok(())
    }

    pub fn insert_table_row_v1(
        &mut self,
        table_id: NodeId,
        index: u32,
        row_id: TableRowId,
        cell_ids: Vec<TableCellId>,
        extent: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        self.create_table_rowcol_operation_v1(
            table_id,
            TableRowColMutationV1::InsertRow {
                index,
                row_id,
                cell_ids,
                extent,
            },
        )
    }

    pub fn delete_table_row_v1(
        &mut self,
        table_id: NodeId,
        row_id: TableRowId,
    ) -> Result<EditOperation, EditorError> {
        self.create_table_rowcol_operation_v1(table_id, TableRowColMutationV1::DeleteRow { row_id })
    }

    pub fn insert_table_column_v1(
        &mut self,
        table_id: NodeId,
        index: u32,
        column_id: TableColumnId,
        cell_ids: Vec<TableCellId>,
        extent: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        self.create_table_rowcol_operation_v1(
            table_id,
            TableRowColMutationV1::InsertColumn {
                index,
                column_id,
                cell_ids,
                extent,
            },
        )
    }

    pub fn delete_table_column_v1(
        &mut self,
        table_id: NodeId,
        column_id: TableColumnId,
    ) -> Result<EditOperation, EditorError> {
        self.create_table_rowcol_operation_v1(
            table_id,
            TableRowColMutationV1::DeleteColumn { column_id },
        )
    }

    fn create_table_rowcol_operation_v1(
        &mut self,
        table_id: NodeId,
        mutation: TableRowColMutationV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.has_node_resize_history_v1(table_id) {
            return Err(EditorError::TableRowColUnsupported { node_id: table_id });
        }
        let story_id = self
            .graph
            .nodes
            .get(&table_id)
            .and_then(|node| node.payload.table.as_ref())
            .and_then(|table| table.story_id)
            .ok_or(EditorError::TableRowColUnsupported { node_id: table_id })?;
        self.validate_table_rowcol_story_metadata_v1(table_id, story_id)?;
        let grid = self
            .current_table_grid_v1(table_id)
            .ok_or(EditorError::TableRowColUnsupported { node_id: table_id })?;
        let bounds = self
            .current_table_bounds_v1(table_id)
            .ok_or(EditorError::TableRowColUnsupported { node_id: table_id })?;
        let before = table_structure_snapshot_from_graph_v1(&self.graph, table_id, grid, bounds)
            .map_err(|_| EditorError::TableRowColUnsupported { node_id: table_id })?;
        let history = canonical_table_rowcol_history_v1(&before, mutation)
            .map_err(|_| EditorError::TableRowColUnsupported { node_id: table_id })?;
        let operation = match history.mutation.clone() {
            TableRowColMutationV1::InsertRow { .. } => EditOperation::InsertTableRow { history },
            TableRowColMutationV1::DeleteRow { .. } => EditOperation::DeleteTableRow { history },
            TableRowColMutationV1::InsertColumn { .. } => {
                EditOperation::InsertTableColumn { history }
            }
            TableRowColMutationV1::DeleteColumn { .. } => {
                EditOperation::DeleteTableColumn { history }
            }
        };
        self.consume_canonical_table_rowcol_operation_v1(operation)
    }

    pub(super) fn consume_canonical_table_rowcol_operation_v1(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let history = table_rowcol_history_v1(&operation)
            .ok_or(EditorError::SourceIdentityChanged)?
            .clone();
        if !table_rowcol_operation_matches_mutation_v1(&operation) {
            return Err(EditorError::TableRowColUnsupported {
                node_id: history.table_id,
            });
        }
        if self.has_node_resize_history_v1(history.table_id) {
            return Err(EditorError::TableRowColUnsupported {
                node_id: history.table_id,
            });
        }
        self.validate_table_rowcol_story_metadata_v1(history.table_id, history.story_id)?;
        let grid = self.current_table_grid_v1(history.table_id).ok_or(
            EditorError::TableRowColUnsupported {
                node_id: history.table_id,
            },
        )?;
        let bounds = self.current_table_bounds_v1(history.table_id).ok_or(
            EditorError::TableRowColUnsupported {
                node_id: history.table_id,
            },
        )?;
        let before =
            table_structure_snapshot_from_graph_v1(&self.graph, history.table_id, grid, bounds)
                .map_err(|_| EditorError::StaleTableRowCol {
                    node_id: history.table_id,
                })?;
        let canonical = canonical_table_rowcol_history_v1(&before, history.mutation.clone())
            .map_err(|_| EditorError::StaleTableRowCol {
                node_id: history.table_id,
            })?;
        if canonical != history {
            return Err(EditorError::StaleTableRowCol {
                node_id: history.table_id,
            });
        }

        let mut candidate_graph = self.graph.clone();
        apply_table_structure_snapshot_to_graph_v1(&mut candidate_graph, &history.after).map_err(
            |_| EditorError::StaleTableRowCol {
                node_id: history.table_id,
            },
        )?;
        self.graph = candidate_graph;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

}
