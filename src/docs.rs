use crate::auth::Credentials;
use crate::errors::{CedarError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    #[serde(rename = "documentId")]
    pub document_id: String,
    pub title: String,
    pub body: DocumentBody,
    pub headers: HashMap<String, Header>,
    pub footers: HashMap<String, Footer>,
    pub footnotes: HashMap<String, Footnote>,
    #[serde(rename = "revisionId")]
    pub revision_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentBody {
    pub content: Vec<StructuralElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralElement {
    #[serde(rename = "startIndex")]
    pub start_index: i32,
    #[serde(rename = "endIndex")]
    pub end_index: i32,
    pub paragraph: Option<Paragraph>,
    pub table: Option<Table>,
    #[serde(rename = "sectionBreak")]
    pub section_break: Option<SectionBreak>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub elements: Vec<ParagraphElement>,
    #[serde(rename = "paragraphStyle")]
    pub paragraph_style: Option<ParagraphStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParagraphElement {
    #[serde(rename = "startIndex")]
    pub start_index: i32,
    #[serde(rename = "endIndex")]
    pub end_index: i32,
    #[serde(rename = "textRun")]
    pub text_run: Option<TextRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRun {
    pub content: String,
    #[serde(rename = "textStyle")]
    pub text_style: Option<TextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    #[serde(rename = "strikethrough")]
    pub strike_through: Option<bool>,
    pub link: Option<Link>,
    #[serde(rename = "fontSize")]
    pub font_size: Option<Dimension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub url: Option<String>,
    #[serde(rename = "headingId")]
    pub heading_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub magnitude: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParagraphStyle {
    #[serde(rename = "namedStyleType")]
    pub named_style_type: Option<String>,
    pub alignment: Option<String>,
    #[serde(rename = "lineSpacing")]
    pub line_spacing: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub rows: i32,
    pub columns: i32,
    #[serde(rename = "tableRows")]
    pub table_rows: Vec<TableRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    #[serde(rename = "tableCells")]
    pub table_cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCell {
    pub content: Vec<StructuralElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionBreak {
    #[serde(rename = "sectionStyle")]
    pub section_style: Option<SectionStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionStyle {
    #[serde(rename = "columnSeparatorStyle")]
    pub column_separator_style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    #[serde(rename = "headerId")]
    pub header_id: String,
    pub content: Vec<StructuralElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Footer {
    #[serde(rename = "footerId")]
    pub footer_id: String,
    pub content: Vec<StructuralElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Footnote {
    #[serde(rename = "footnoteId")]
    pub footnote_id: String,
    pub content: Vec<StructuralElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUpdateRequest {
    pub requests: Vec<Request>,
    #[serde(rename = "writeControl")]
    pub write_control: Option<WriteControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteControl {
    #[serde(rename = "requiredRevisionId")]
    pub required_revision_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Request {
    InsertText(InsertTextRequest),
    DeleteContentRange(DeleteContentRangeRequest),
    UpdateTextStyle(UpdateTextStyleRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertTextRequest {
    #[serde(rename = "insertText")]
    pub insert_text: InsertText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertText {
    pub location: Location,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteContentRangeRequest {
    #[serde(rename = "deleteContentRange")]
    pub delete_content_range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTextStyleRequest {
    #[serde(rename = "updateTextStyle")]
    pub update_text_style: UpdateTextStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTextStyle {
    pub range: Range,
    #[serde(rename = "textStyle")]
    pub text_style: TextStyle,
    pub fields: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub index: i32,
    #[serde(rename = "segmentId")]
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    #[serde(rename = "startIndex")]
    pub start_index: i32,
    #[serde(rename = "endIndex")]
    pub end_index: i32,
    #[serde(rename = "segmentId")]
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUpdateResponse {
    #[serde(rename = "documentId")]
    pub document_id: String,
    #[serde(rename = "writeControl")]
    pub write_control: Option<WriteControl>,
    pub replies: Vec<serde_json::Value>,
}

pub enum SuggestionsMode {
    SuggestionsInline,
    PreviewSuggestionsAccepted,
    PreviewWithoutSuggestions,
}

impl SuggestionsMode {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            Self::SuggestionsInline => "SUGGESTIONS_INLINE",
            Self::PreviewSuggestionsAccepted => "PREVIEW_SUGGESTIONS_ACCEPTED",
            Self::PreviewWithoutSuggestions => "PREVIEW_WITHOUT_SUGGESTIONS",
        }
    }
}

#[derive(Clone)]
pub struct GoogleDocsClient {
    client: Client,
    base_url: String,
}

impl GoogleDocsClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://docs.googleapis.com/v1".to_string(),
        }
    }

    pub async fn get_document(
        &self,
        document_id: &str,
        credentials: &Credentials,
        suggestions_mode: Option<SuggestionsMode>,
    ) -> Result<Document> {
        let mut url = format!("{}/documents/{}", self.base_url, document_id);

        if let Some(mode) = suggestions_mode {
            url += &format!("?suggestionsViewMode={}", mode.as_query_param());
        }

        let response = self
            .client
            .get(&url)
            .bearer_auth(&credentials.access_token)
            .send()
            .await
            .map_err(|e| CedarError::GoogleDocs(format!("Failed to get document: {}", e)))?;

        let status = response.status();
        let headers = response.headers().clone();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDocs(format!(
                "API request failed with status {}: {}",
                status, body
            )));
        }

        let document: Document = response.json().await.map_err(|e| {
            CedarError::GoogleDocs(format!("Failed to parse document response: {}", e))
        })?;

        debug!("Retrieved document: {} (revision: {})", document.title, document.revision_id);
        Ok(document)
    }

    pub async fn batch_update(
        &self,
        document_id: &str,
        request: BatchUpdateRequest,
        credentials: &Credentials,
    ) -> Result<BatchUpdateResponse> {
        let url = format!("{}/documents/{}:batchUpdate", self.base_url, document_id);

        debug!(
            "Sending batch update with {} requests (revision: {:?})",
            request.requests.len(),
            request.write_control.as_ref().map(|wc| &wc.required_revision_id)
        );

        let response = self
            .client
            .post(&url)
            .bearer_auth(&credentials.access_token)
            .json(&request)
            .send()
            .await
            .map_err(|e| CedarError::GoogleDocs(format!("Failed to send batch update: {}", e)))?;

        let status = response.status();
        let headers = response.headers().clone();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            
            if status == reqwest::StatusCode::BAD_REQUEST && body.contains("INVALID_REVISION_ID") {
                return Err(CedarError::Conflict(
                    "Document was modified by another client, revision ID mismatch".to_string()
                ));
            }
            
            return Err(CedarError::GoogleDocs(format!(
                "Batch update failed with status {}: {}",
                status, body
            )));
        }

        let batch_response: BatchUpdateResponse = response.json().await.map_err(|e| {
            CedarError::GoogleDocs(format!("Failed to parse batch update response: {}", e))
        })?;

        info!("Batch update completed successfully");
        Ok(batch_response)
    }
}