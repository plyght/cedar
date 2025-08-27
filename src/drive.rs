use crate::auth::Credentials;
use crate::errors::{CedarError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResource {
    pub id: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub parents: Option<Vec<String>>,
    pub version: Option<String>,
    #[serde(rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(rename = "lastModifyingUser")]
    pub last_modifying_user: Option<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "emailAddress")]
    pub email_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    #[serde(rename = "changeType")]
    pub change_type: String,
    pub time: String,
    pub removed: Option<bool>,
    pub file: Option<FileResource>,
    #[serde(rename = "fileId")]
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeList {
    pub kind: String,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
    #[serde(rename = "newStartPageToken")]
    pub new_start_page_token: Option<String>,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPageToken {
    pub kind: String,
    #[serde(rename = "startPageToken")]
    pub start_page_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub watch_type: String,
    pub address: String,
    pub token: Option<String>,
    pub expiration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchResponse {
    pub kind: String,
    pub id: String,
    #[serde(rename = "resourceId")]
    pub resource_id: String,
    #[serde(rename = "resourceUri")]
    pub resource_uri: String,
    pub token: Option<String>,
    pub expiration: Option<String>,
}

#[derive(Clone)]
pub struct GoogleDriveClient {
    client: Client,
    base_url: String,
}

impl GoogleDriveClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://www.googleapis.com/drive/v3".to_string(),
        }
    }

    pub async fn get_file(&self, file_id: &str, credentials: &Credentials) -> Result<FileResource> {
        let url = format!("{}/files/{}", self.base_url, file_id);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&credentials.access_token)
            .query(&[(
                "fields",
                "id,name,mimeType,parents,version,modifiedTime,lastModifyingUser",
            )])
            .send()
            .await
            .map_err(|e| CedarError::GoogleDrive(format!("Failed to get file: {e}")))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "Get file failed with status {status}: {body}"
            )));
        }

        let file: FileResource = response.json().await.map_err(|e| {
            CedarError::GoogleDrive(format!("Failed to parse file response: {e}"))
        })?;

        debug!("Retrieved file metadata: {} ({})", file.name, file.id);
        Ok(file)
    }

    pub async fn get_start_page_token(&self, credentials: &Credentials) -> Result<String> {
        let url = format!("{}/changes/startPageToken", self.base_url);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&credentials.access_token)
            .send()
            .await
            .map_err(|e| {
                CedarError::GoogleDrive(format!("Failed to get start page token: {e}"))
            })?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "Get start page token failed with status {status}: {body}"
            )));
        }

        let token_response: StartPageToken = response.json().await.map_err(|e| {
            CedarError::GoogleDrive(format!("Failed to parse start page token response: {e}"))
        })?;

        debug!(
            "Retrieved start page token: {}",
            token_response.start_page_token
        );
        Ok(token_response.start_page_token)
    }

    pub async fn list_changes(
        &self,
        page_token: &str,
        credentials: &Credentials,
    ) -> Result<ChangeList> {
        let url = format!("{}/changes", self.base_url);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&credentials.access_token)
            .query(&[
                ("pageToken", page_token),
                (
                    "fields",
                    "nextPageToken,newStartPageToken,changes(changeType,time,removed,file,fileId)",
                ),
            ])
            .send()
            .await
            .map_err(|e| CedarError::GoogleDrive(format!("Failed to list changes: {e}")))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "List changes failed with status {status}: {body}"
            )));
        }

        let changes: ChangeList = response.json().await.map_err(|e| {
            CedarError::GoogleDrive(format!("Failed to parse changes response: {e}"))
        })?;

        debug!("Retrieved {} changes", changes.changes.len());
        Ok(changes)
    }

    pub async fn watch_changes(
        &self,
        page_token: &str,
        webhook_url: &str,
        credentials: &Credentials,
    ) -> Result<WatchResponse> {
        let url = format!("{}/changes/watch", self.base_url);

        let watch_request = WatchRequest {
            id: uuid::Uuid::new_v4().to_string(),
            watch_type: "web_hook".to_string(),
            address: webhook_url.to_string(),
            token: None,
            expiration: None,
        };

        let response = self
            .client
            .post(&url)
            .bearer_auth(&credentials.access_token)
            .query(&[("pageToken", page_token)])
            .json(&watch_request)
            .send()
            .await
            .map_err(|e| CedarError::GoogleDrive(format!("Failed to watch changes: {e}")))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "Watch changes failed with status {status}: {body}"
            )));
        }

        let watch_response: WatchResponse = response.json().await.map_err(|e| {
            CedarError::GoogleDrive(format!("Failed to parse watch response: {e}"))
        })?;

        info!(
            "Set up change watch for resource {} (expires: {:?})",
            watch_response.resource_id, watch_response.expiration
        );
        Ok(watch_response)
    }

    pub async fn stop_watch(
        &self,
        channel_id: &str,
        resource_id: &str,
        credentials: &Credentials,
    ) -> Result<()> {
        let url = format!("{}/channels/stop", self.base_url);

        let stop_request = serde_json::json!({
            "id": channel_id,
            "resourceId": resource_id
        });

        let response = self
            .client
            .post(&url)
            .bearer_auth(&credentials.access_token)
            .json(&stop_request)
            .send()
            .await
            .map_err(|e| CedarError::GoogleDrive(format!("Failed to stop watch: {e}")))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "Stop watch failed with status {status}: {body}"
            )));
        }

        info!("Stopped change watch for channel {}", channel_id);
        Ok(())
    }

    pub async fn export_document(
        &self,
        document_id: &str,
        mime_type: &str,
        credentials: &Credentials,
    ) -> Result<bytes::Bytes> {
        let url = format!("{}/files/{}/export", self.base_url, document_id);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&credentials.access_token)
            .query(&[("mimeType", mime_type)])
            .send()
            .await
            .map_err(|e| CedarError::GoogleDrive(format!("Failed to export document: {e}")))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(CedarError::RateLimit {
                retry_after_seconds: retry_after,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CedarError::GoogleDrive(format!(
                "Export document failed with status {status}: {body}"
            )));
        }

        let content = response.bytes().await.map_err(|e| {
            CedarError::GoogleDrive(format!("Failed to read export content: {e}"))
        })?;

        info!("Exported document {} as {}", document_id, mime_type);
        Ok(content)
    }
}
