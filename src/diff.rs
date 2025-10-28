use crate::docs::{
    BatchUpdateRequest, DeleteContentRangeRequest, InsertText, InsertTextRequest, Location, Range,
    Request, WriteControl,
};
use crate::errors::{CedarError, Result};
use similar::{ChangeTag, TextDiff};
use std::cmp;
use tracing::{debug, warn};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start_index: usize,
    pub end_index: usize,
    pub new_text: String,
    pub operation: EditOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOperation {
    Insert,
    Delete,
    Replace,
}

pub struct DiffCalculator {
    utf16_offset_cache: std::collections::HashMap<String, Vec<usize>>,
}

impl Default for DiffCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            utf16_offset_cache: std::collections::HashMap::new(),
        }
    }

    pub fn calculate_edits(&mut self, old_text: &str, new_text: &str) -> Result<Vec<TextEdit>> {
        debug!(
            "Calculating diff between texts of lengths {} and {}",
            old_text.len(),
            new_text.len()
        );

        let diff = TextDiff::from_lines(old_text, new_text);
        let mut edits = Vec::new();
        let mut old_pos = 0;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    old_pos += change.value().len();
                }
                ChangeTag::Delete => {
                    let start = old_pos;
                    let end = old_pos + change.value().len();

                    edits.push(TextEdit {
                        start_index: start,
                        end_index: end,
                        new_text: String::new(),
                        operation: EditOperation::Delete,
                    });

                    old_pos += change.value().len();
                }
                ChangeTag::Insert => {
                    edits.push(TextEdit {
                        start_index: old_pos,
                        end_index: old_pos,
                        new_text: change.value().to_string(),
                        operation: EditOperation::Insert,
                    });
                }
            }
        }

        let merged_edits = self.merge_adjacent_edits(edits)?;
        debug!("Generated {} edits after merging", merged_edits.len());

        Ok(merged_edits)
    }

    fn merge_adjacent_edits(&self, mut edits: Vec<TextEdit>) -> Result<Vec<TextEdit>> {
        if edits.is_empty() {
            return Ok(edits);
        }

        edits.sort_by_key(|edit| edit.start_index);

        let mut merged = Vec::new();
        let mut current = edits[0].clone();

        for edit in edits.into_iter().skip(1) {
            if current.end_index == edit.start_index {
                match (&current.operation, &edit.operation) {
                    (EditOperation::Delete, EditOperation::Insert) => {
                        current = TextEdit {
                            start_index: current.start_index,
                            end_index: current.end_index,
                            new_text: edit.new_text,
                            operation: EditOperation::Replace,
                        };
                    }
                    (EditOperation::Insert, EditOperation::Delete) => {
                        warn!("Unexpected insert followed by delete at same position");
                        merged.push(current);
                        current = edit;
                    }
                    (EditOperation::Delete, EditOperation::Delete) => {
                        current.end_index = edit.end_index;
                    }
                    (EditOperation::Insert, EditOperation::Insert) => {
                        current.new_text.push_str(&edit.new_text);
                    }
                    _ => {
                        merged.push(current);
                        current = edit;
                    }
                }
            } else {
                merged.push(current);
                current = edit;
            }
        }

        merged.push(current);
        Ok(merged)
    }

    pub fn convert_to_batch_update(
        &mut self,
        edits: Vec<TextEdit>,
        old_text: &str,
        revision_id: &str,
        segment_id: Option<String>,
    ) -> Result<BatchUpdateRequest> {
        debug!("Converting {} edits to batch update", edits.len());

        let utf16_offsets = self.calculate_utf16_offsets(old_text)?;
        let mut requests = Vec::new();

        let mut sorted_edits = edits;
        sorted_edits.sort_by_key(|edit| cmp::Reverse(edit.start_index));

        for edit in sorted_edits {
            let utf16_start = self.utf8_to_utf16_index(edit.start_index, &utf16_offsets)?;
            let utf16_end = self.utf8_to_utf16_index(edit.end_index, &utf16_offsets)?;

            match edit.operation {
                EditOperation::Delete => {
                    requests.push(Request::DeleteContentRange(DeleteContentRangeRequest {
                        delete_content_range: Range {
                            start_index: utf16_start as i32,
                            end_index: utf16_end as i32,
                            segment_id: segment_id.clone(),
                        },
                    }));
                }
                EditOperation::Insert => {
                    requests.push(Request::InsertText(InsertTextRequest {
                        insert_text: InsertText {
                            location: Location {
                                index: utf16_start as i32,
                                segment_id: segment_id.clone(),
                            },
                            text: edit.new_text,
                        },
                    }));
                }
                EditOperation::Replace => {
                    requests.push(Request::DeleteContentRange(DeleteContentRangeRequest {
                        delete_content_range: Range {
                            start_index: utf16_start as i32,
                            end_index: utf16_end as i32,
                            segment_id: segment_id.clone(),
                        },
                    }));

                    requests.push(Request::InsertText(InsertTextRequest {
                        insert_text: InsertText {
                            location: Location {
                                index: utf16_start as i32,
                                segment_id: segment_id.clone(),
                            },
                            text: edit.new_text,
                        },
                    }));
                }
            }
        }

        Ok(BatchUpdateRequest {
            requests,
            write_control: Some(WriteControl {
                required_revision_id: revision_id.to_string(),
            }),
        })
    }

    fn calculate_utf16_offsets(&mut self, text: &str) -> Result<Vec<usize>> {
        if let Some(cached) = self.utf16_offset_cache.get(text) {
            return Ok(cached.clone());
        }

        let mut offsets = Vec::new();
        let mut utf16_pos = 0;

        for grapheme in text.graphemes(true) {
            offsets.push(utf16_pos);
            utf16_pos += grapheme.encode_utf16().count();
        }
        offsets.push(utf16_pos);

        if self.utf16_offset_cache.len() > 100 {
            self.utf16_offset_cache.clear();
        }

        self.utf16_offset_cache
            .insert(text.to_string(), offsets.clone());
        Ok(offsets)
    }

    fn utf8_to_utf16_index(&self, utf8_index: usize, utf16_offsets: &[usize]) -> Result<usize> {
        if utf8_index >= utf16_offsets.len() {
            return Err(CedarError::IndexOutOfBounds(format!(
                "UTF-8 index {} exceeds text length {}",
                utf8_index,
                utf16_offsets.len() - 1
            )));
        }

        Ok(utf16_offsets[utf8_index])
    }

    // Helper method for applying remote changes - useful for future conflict resolution features
    #[allow(dead_code)]
    pub fn apply_remote_changes(
        &self,
        local_text: &str,
        remote_changes: &[TextEdit],
    ) -> Result<String> {
        debug!(
            "Applying {} remote changes to local text",
            remote_changes.len()
        );

        let mut result = local_text.to_string();
        let mut sorted_changes = remote_changes.to_vec();
        sorted_changes.sort_by_key(|change| cmp::Reverse(change.start_index));

        for change in sorted_changes {
            if change.start_index > result.len() || change.end_index > result.len() {
                return Err(CedarError::IndexOutOfBounds(format!(
                    "Change indices ({}, {}) exceed text length {}",
                    change.start_index,
                    change.end_index,
                    result.len()
                )));
            }

            match change.operation {
                EditOperation::Delete => {
                    result.drain(change.start_index..change.end_index);
                }
                EditOperation::Insert => {
                    result.insert_str(change.start_index, &change.new_text);
                }
                EditOperation::Replace => {
                    result.drain(change.start_index..change.end_index);
                    result.insert_str(change.start_index, &change.new_text);
                }
            }
        }

        Ok(result)
    }

    pub fn detect_conflicts(
        &self,
        local_edits: &[TextEdit],
        remote_edits: &[TextEdit],
    ) -> Vec<ConflictRegion> {
        debug!(
            "Detecting conflicts between {} local and {} remote edits",
            local_edits.len(),
            remote_edits.len()
        );

        let mut conflicts = Vec::new();

        for local_edit in local_edits {
            for remote_edit in remote_edits {
                if self.edits_overlap(local_edit, remote_edit) {
                    conflicts.push(ConflictRegion {
                        start_index: cmp::min(local_edit.start_index, remote_edit.start_index),
                        end_index: cmp::max(local_edit.end_index, remote_edit.end_index),
                        local_edit: local_edit.clone(),
                        remote_edit: remote_edit.clone(),
                    });
                }
            }
        }

        debug!("Detected {} conflicts", conflicts.len());
        conflicts
    }

    fn edits_overlap(&self, edit1: &TextEdit, edit2: &TextEdit) -> bool {
        !(edit1.end_index <= edit2.start_index || edit2.end_index <= edit1.start_index)
    }
}

#[derive(Debug, Clone)]
pub struct ConflictRegion {
    pub start_index: usize,
    pub end_index: usize,
    pub local_edit: TextEdit,
    pub remote_edit: TextEdit,
}

impl ConflictRegion {
    pub fn format_conflict_marker(&self, original_text: &str) -> String {
        let original_section =
            if self.start_index < original_text.len() && self.end_index <= original_text.len() {
                &original_text[self.start_index..self.end_index]
            } else {
                "[content unavailable]"
            };

        format!(
            "<<<<<<< LOCAL\n{}\n=======\n{}\n>>>>>>> REMOTE ({})\n",
            self.local_edit.new_text, self.remote_edit.new_text, original_section
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_insert() {
        let mut calc = DiffCalculator::new();
        let old_text = "Hello world\n";
        let new_text = "Hello world\nNew line\n";

        let edits = calc.calculate_edits(old_text, new_text).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].operation, EditOperation::Insert);
        assert_eq!(edits[0].new_text, "New line\n");
    }

    #[test]
    fn test_simple_delete() {
        let mut calc = DiffCalculator::new();
        let old_text = "Hello world\nExtra line\n";
        let new_text = "Hello world\n";

        let edits = calc.calculate_edits(old_text, new_text).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].operation, EditOperation::Delete);
    }

    #[test]
    fn test_utf16_offset_calculation() {
        let mut calc = DiffCalculator::new();
        let text = "Hello 😄 world";
        let offsets = calc.calculate_utf16_offsets(text).unwrap();

        assert!(!offsets.is_empty());
        assert_eq!(offsets[0], 0);
        // Text has 13 characters (graphemes), but UTF-16 length is different:
        // "Hello" (5) + " " (1) + "😄" (1 char = 2 UTF-16 units) + " " (1) + "world" (5)
        // Total UTF-16 code units: 5 + 1 + 2 + 1 + 5 = 14
        assert_eq!(*offsets.last().unwrap(), 14);
    }

    #[test]
    fn test_conflict_detection() {
        let calc = DiffCalculator::new();

        let local_edits = vec![TextEdit {
            start_index: 5,
            end_index: 10,
            new_text: "local".to_string(),
            operation: EditOperation::Replace,
        }];

        let remote_edits = vec![TextEdit {
            start_index: 7,
            end_index: 12,
            new_text: "remote".to_string(),
            operation: EditOperation::Replace,
        }];

        let conflicts = calc.detect_conflicts(&local_edits, &remote_edits);
        assert_eq!(conflicts.len(), 1);
    }
}
