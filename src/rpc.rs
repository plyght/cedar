use crate::auth::{AuthManager, Credentials};
use crate::config::Config;
use crate::diff::{ConflictRegion, DiffCalculator};
use crate::docs::{GoogleDocsClient, SuggestionsMode};
use crate::drive::GoogleDriveClient;
use crate::errors::{CedarError, Result};
use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
    server::{ServerBuilder, ServerHandle},
    types::{ErrorCode, ErrorObject},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub id: String,
    pub title: String,
    pub revision_id: String,
    pub content: String,
    pub suggestions_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub document_id: String,
    pub local_revision: String,
    pub remote_revision: String,
    pub last_sync: chrono::DateTime<chrono::Utc>,
    pub conflicts: Vec<ConflictInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub start_index: usize,
    pub end_index: usize,
    pub local_content: String,
    pub remote_content: String,
    pub marker: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextChange {
    pub document_id: String,
    pub content: String,
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub document_id: String,
    pub format: String, // "docx", "pdf", "md", "txt"
}

#[rpc(server)]
pub trait CedarRpc {
    #[method(name = "authenticate")]
    async fn authenticate(&self) -> RpcResult<bool>;

    #[method(name = "open_document")]
    async fn open_document(
        &self,
        document_id: String,
        suggestions_mode: Option<String>,
    ) -> RpcResult<DocumentInfo>;

    #[method(name = "sync_document")]
    async fn sync_document(&self, change: TextChange) -> RpcResult<SyncStatus>;

    #[method(name = "get_sync_status")]
    async fn get_sync_status(&self, document_id: String) -> RpcResult<SyncStatus>;

    #[method(name = "resolve_conflicts")]
    async fn resolve_conflicts(&self, document_id: String, resolution: String) -> RpcResult<bool>;

    #[method(name = "export_document")]
    async fn export_document(&self, request: ExportRequest) -> RpcResult<Vec<u8>>;

    #[method(name = "list_documents")]
    async fn list_documents(&self) -> RpcResult<Vec<DocumentInfo>>;

    #[method(name = "get_health")]
    async fn get_health(&self) -> RpcResult<String>;
}

#[derive(Clone)]
pub struct CedarRpcImpl {
    config: Config,
    auth_manager: Arc<Mutex<AuthManager>>,
    docs_client: GoogleDocsClient,
    drive_client: GoogleDriveClient,
    diff_calculator: Arc<Mutex<DiffCalculator>>,
    document_cache: Arc<RwLock<HashMap<String, DocumentInfo>>>,
    sync_statuses: Arc<RwLock<HashMap<String, SyncStatus>>>,
    credentials: Arc<RwLock<Option<Credentials>>>,
}

impl CedarRpcImpl {
    fn make_error(message: String) -> ErrorObject<'static> {
        ErrorObject::owned(ErrorCode::InternalError.code(), message, None::<()>)
    }

    pub fn new(config: Config) -> Result<Self> {
        let auth_manager = AuthManager::new(config.google.clone())?;

        Ok(Self {
            config,
            auth_manager: Arc::new(Mutex::new(auth_manager)),
            docs_client: GoogleDocsClient::new(),
            drive_client: GoogleDriveClient::new(),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            document_cache: Arc::new(RwLock::new(HashMap::new())),
            sync_statuses: Arc::new(RwLock::new(HashMap::new())),
            credentials: Arc::new(RwLock::new(None)),
        })
    }

    async fn ensure_authenticated(&self) -> Result<Credentials> {
        let credentials_guard = self.credentials.read().await;
        if let Some(creds) = credentials_guard.as_ref() {
            if !self.is_token_expired(creds) {
                return Ok(creds.clone());
            }
        }
        drop(credentials_guard);

        let mut auth_manager = self.auth_manager.lock().await;
        let fresh_credentials = auth_manager.get_valid_credentials().await?;

        let mut credentials_guard = self.credentials.write().await;
        *credentials_guard = Some(fresh_credentials.clone());

        Ok(fresh_credentials)
    }

    fn is_token_expired(&self, credentials: &Credentials) -> bool {
        credentials.expires_at.is_some_and(|exp| {
            chrono::Utc::now() + chrono::Duration::minutes(5) > exp
        })
    }

    async fn convert_document_to_text(&self, document: &crate::docs::Document) -> String {
        let mut content = String::new();

        for element in &document.body.content {
            if let Some(paragraph) = &element.paragraph {
                for para_element in &paragraph.elements {
                    if let Some(text_run) = &para_element.text_run {
                        content.push_str(&text_run.content);
                    }
                }
            }
        }

        content
    }

    fn suggestions_mode_from_string(&self, mode: &str) -> SuggestionsMode {
        match mode.to_lowercase().as_str() {
            "inline" => SuggestionsMode::SuggestionsInline,
            "accepted" => SuggestionsMode::PreviewSuggestionsAccepted,
            "without" | "rejected" => SuggestionsMode::PreviewWithoutSuggestions,
            _ => SuggestionsMode::SuggestionsInline,
        }
    }

    fn convert_conflicts(
        &self,
        conflicts: Vec<ConflictRegion>,
        original_text: &str,
    ) -> Vec<ConflictInfo> {
        conflicts
            .into_iter()
            .map(|conflict| {
                let marker = conflict.format_conflict_marker(original_text);
                ConflictInfo {
                    start_index: conflict.start_index,
                    end_index: conflict.end_index,
                    local_content: conflict.local_edit.new_text,
                    remote_content: conflict.remote_edit.new_text,
                    marker,
                }
            })
            .collect()
    }
}

#[async_trait]
impl CedarRpcServer for CedarRpcImpl {
    async fn authenticate(&self) -> RpcResult<bool> {
        match self.ensure_authenticated().await {
            Ok(_) => {
                info!("Authentication successful");
                Ok(true)
            }
            Err(e) => {
                error!("Authentication failed: {}", e);
                Err(Self::make_error(format!("Authentication failed: {e}")))
            }
        }
    }

    async fn open_document(
        &self,
        document_id: String,
        suggestions_mode: Option<String>,
    ) -> RpcResult<DocumentInfo> {
        let credentials = self
            .ensure_authenticated()
            .await
            .map_err(|e| Self::make_error(format!("Authentication failed: {e}")))?;

        let mode = suggestions_mode
            .as_ref()
            .map(|m| self.suggestions_mode_from_string(m));

        let document = self
            .docs_client
            .get_document(&document_id, &credentials, mode)
            .await
            .map_err(|e| Self::make_error(format!("Failed to get document: {e}")))?;

        let content = self.convert_document_to_text(&document).await;

        let doc_info = DocumentInfo {
            id: document.document_id.clone(),
            title: document.title.clone(),
            revision_id: document.revision_id.clone(),
            content,
            suggestions_mode: suggestions_mode.unwrap_or_else(|| "inline".to_string()),
        };

        self.document_cache
            .write()
            .await
            .insert(document_id.clone(), doc_info.clone());

        let sync_status = SyncStatus {
            document_id: document_id.clone(),
            local_revision: document.revision_id.clone(),
            remote_revision: document.revision_id,
            last_sync: chrono::Utc::now(),
            conflicts: vec![],
        };

        self.sync_statuses
            .write()
            .await
            .insert(document_id, sync_status);

        info!("Opened document: {} ({})", doc_info.title, doc_info.id);
        Ok(doc_info)
    }

    async fn sync_document(&self, change: TextChange) -> RpcResult<SyncStatus> {
        let credentials = self
            .ensure_authenticated()
            .await
            .map_err(|e| Self::make_error(format!("Authentication failed: {e}")))?;

        let cached_doc = self
            .document_cache
            .read()
            .await
            .get(&change.document_id)
            .cloned();
        let cached_doc =
            cached_doc.ok_or_else(|| Self::make_error("Document not found in cache. Call open_document first.".to_string()))?;

        let current_document = self
            .docs_client
            .get_document(&change.document_id, &credentials, None)
            .await
            .map_err(|e| Self::make_error(format!("Failed to get current document: {e}")))?;

        let current_content = self.convert_document_to_text(&current_document).await;

        if current_document.revision_id != cached_doc.revision_id {
            warn!("Document was modified remotely, handling conflict");

            let mut diff_calc = self.diff_calculator.lock().await;
            let local_edits = diff_calc
                .calculate_edits(&cached_doc.content, &change.content)
                .map_err(|e| Self::make_error(format!("Failed to calculate local edits: {e}")))?;

            let remote_edits = diff_calc
                .calculate_edits(&cached_doc.content, &current_content)
                .map_err(|e| Self::make_error(format!("Failed to calculate remote edits: {e}")))?;

            let conflicts = diff_calc.detect_conflicts(&local_edits, &remote_edits);

            if !conflicts.is_empty() {
                let conflict_infos = self.convert_conflicts(conflicts, &cached_doc.content);

                let sync_status = SyncStatus {
                    document_id: change.document_id.clone(),
                    local_revision: cached_doc.revision_id,
                    remote_revision: current_document.revision_id,
                    last_sync: chrono::Utc::now(),
                    conflicts: conflict_infos,
                };

                self.sync_statuses
                    .write()
                    .await
                    .insert(change.document_id.clone(), sync_status.clone());
                return Ok(sync_status);
            }
        }

        let mut diff_calc = self.diff_calculator.lock().await;
        let edits = diff_calc
            .calculate_edits(&cached_doc.content, &change.content)
            .map_err(|e| Self::make_error(format!("Failed to calculate diff: {e}")))?;

        if edits.is_empty() {
            debug!("No changes detected, skipping sync");
            let sync_status = self
                .sync_statuses
                .read()
                .await
                .get(&change.document_id)
                .cloned()
                .unwrap_or_else(|| SyncStatus {
                    document_id: change.document_id,
                    local_revision: cached_doc.revision_id,
                    remote_revision: current_document.revision_id,
                    last_sync: chrono::Utc::now(),
                    conflicts: vec![],
                });
            return Ok(sync_status);
        }

        let batch_request = diff_calc
            .convert_to_batch_update(
                edits,
                &cached_doc.content,
                &current_document.revision_id,
                None,
            )
            .map_err(|e| Self::make_error(format!("Failed to create batch update: {e}")))?;

        drop(diff_calc);

        match self
            .docs_client
            .batch_update(&change.document_id, batch_request, &credentials)
            .await
        {
            Ok(response) => {
                let updated_doc = DocumentInfo {
                    id: change.document_id.clone(),
                    title: cached_doc.title,
                    revision_id: response
                        .write_control
                        .map(|wc| wc.required_revision_id)
                        .unwrap_or(current_document.revision_id),
                    content: change.content.clone(),
                    suggestions_mode: cached_doc.suggestions_mode,
                };

                self.document_cache
                    .write()
                    .await
                    .insert(change.document_id.clone(), updated_doc.clone());

                let sync_status = SyncStatus {
                    document_id: change.document_id.clone(),
                    local_revision: updated_doc.revision_id.clone(),
                    remote_revision: updated_doc.revision_id,
                    last_sync: chrono::Utc::now(),
                    conflicts: vec![],
                };

                self.sync_statuses
                    .write()
                    .await
                    .insert(change.document_id, sync_status.clone());

                info!("Document synced successfully");
                Ok(sync_status)
            }
            Err(CedarError::Conflict(msg)) => {
                warn!("Sync conflict detected: {}", msg);

                let sync_status = SyncStatus {
                    document_id: change.document_id.clone(),
                    local_revision: cached_doc.revision_id,
                    remote_revision: current_document.revision_id,
                    last_sync: chrono::Utc::now(),
                    conflicts: vec![ConflictInfo {
                        start_index: 0,
                        end_index: change.content.len(),
                        local_content: change.content.clone(),
                        remote_content: current_content.clone(),
                        marker: format!(
                            "<<<<<<< LOCAL\n{}\n=======\n{}\n>>>>>>> REMOTE\n",
                            change.content, current_content
                        ),
                    }],
                };

                self.sync_statuses
                    .write()
                    .await
                    .insert(change.document_id, sync_status.clone());
                Ok(sync_status)
            }
            Err(e) => {
                error!("Batch update failed: {}", e);
                Err(Self::make_error(format!("Sync failed: {e}")))
            }
        }
    }

    async fn get_sync_status(&self, document_id: String) -> RpcResult<SyncStatus> {
        let sync_status = self
            .sync_statuses
            .read()
            .await
            .get(&document_id)
            .cloned()
            .ok_or_else(|| Self::make_error("Document not found. Call open_document first.".to_string()))?;

        Ok(sync_status)
    }

    async fn resolve_conflicts(&self, document_id: String, resolution: String) -> RpcResult<bool> {
        let mut sync_statuses = self.sync_statuses.write().await;
        if let Some(status) = sync_statuses.get_mut(&document_id) {
            if !status.conflicts.is_empty() {
                status.conflicts.clear();

                if let Some(cached_doc) =
                    self.document_cache.write().await.get_mut(&document_id)
                {
                    cached_doc.content = resolution;
                }

                info!("Conflicts resolved for document {}", document_id);
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(Self::make_error("Document not found".to_string()))
        }
    }

    async fn export_document(&self, request: ExportRequest) -> RpcResult<Vec<u8>> {
        let credentials = self
            .ensure_authenticated()
            .await
            .map_err(|e| Self::make_error(format!("Authentication failed: {e}")))?;

        let mime_type = match request.format.as_str() {
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "md" => "text/plain",
            _ => return Err(Self::make_error("Unsupported export format".to_string())),
        };

        let content = self
            .drive_client
            .export_document(&request.document_id, mime_type, &credentials)
            .await
            .map_err(|e| Self::make_error(format!("Export failed: {e}")))?;

        info!(
            "Exported document {} as {}",
            request.document_id, request.format
        );
        Ok(content.to_vec())
    }

    async fn list_documents(&self) -> RpcResult<Vec<DocumentInfo>> {
        let documents = self.document_cache.read().await.values().cloned().collect();

        Ok(documents)
    }

    async fn get_health(&self) -> RpcResult<String> {
        let auth_status = match self.credentials.read().await.as_ref() {
            Some(creds) if !self.is_token_expired(creds) => "authenticated",
            _ => "unauthenticated",
        };

        let doc_count = self.document_cache.read().await.len();

        Ok(format!(
            "Cedar daemon is running. Auth: {auth_status}, Documents: {doc_count}"
        ))
    }
}

pub struct JsonRpcServer {
    handle: Option<ServerHandle>,
    address: SocketAddr,
    rpc_impl: Arc<CedarRpcImpl>,
}

impl JsonRpcServer {
    pub async fn new(config: Config) -> Result<Self> {
        let rpc_impl = Arc::new(CedarRpcImpl::new(config.clone())?);
        let address: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
            .parse()
            .map_err(|e| CedarError::Config(format!("Invalid server address: {e}")))?;

        Ok(Self {
            handle: None,
            address,
            rpc_impl,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub async fn run(&mut self) -> Result<()> {
        let server = ServerBuilder::default()
            .build(&self.address)
            .await
            .map_err(|e| CedarError::JsonRpc(format!("Failed to build server: {e}")))?;

        let addr = server
            .local_addr()
            .map_err(|e| CedarError::JsonRpc(format!("Failed to get server address: {e}")))?;

        let handle = server.start(self.rpc_impl.as_ref().clone().into_rpc());
        self.handle = Some(handle.clone());

        info!("JSON-RPC server listening on {}", addr);

        self.start_background_tasks().await;

        handle.stopped().await;
        Ok(())
    }

    async fn start_background_tasks(&self) {
        let rpc_impl = Arc::clone(&self.rpc_impl);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(
                rpc_impl.config.sync.poll_interval_seconds,
            ));

            loop {
                interval.tick().await;

                if let Err(e) = Self::poll_for_changes(&rpc_impl).await {
                    warn!("Failed to poll for changes: {}", e);
                }
            }
        });
    }

    async fn poll_for_changes(rpc_impl: &CedarRpcImpl) -> Result<()> {
        let credentials_guard = rpc_impl.credentials.read().await;
        let Some(credentials) = credentials_guard.as_ref() else {
            return Ok(()); // Not authenticated yet
        };

        if rpc_impl.is_token_expired(credentials) {
            debug!("Skipping change poll - token expired");
            return Ok(());
        }

        let credentials = credentials.clone();
        drop(credentials_guard);

        let document_ids: Vec<String> = rpc_impl
            .document_cache
            .read()
            .await
            .keys()
            .cloned()
            .collect();

        for document_id in document_ids {
            match Self::check_document_changes(rpc_impl, &document_id, &credentials).await {
                Ok(changed) => {
                    if changed {
                        info!("Remote changes detected for document {}", document_id);
                    }
                }
                Err(e) => {
                    debug!(
                        "Failed to check changes for document {}: {}",
                        document_id, e
                    );
                }
            }
        }

        Ok(())
    }

    async fn check_document_changes(
        rpc_impl: &CedarRpcImpl,
        document_id: &str,
        credentials: &Credentials,
    ) -> Result<bool> {
        let cached_doc = rpc_impl
            .document_cache
            .read()
            .await
            .get(document_id)
            .cloned();

        let Some(cached_doc) = cached_doc else {
            return Ok(false);
        };

        let current_document = rpc_impl
            .docs_client
            .get_document(document_id, credentials, None)
            .await?;

        if current_document.revision_id != cached_doc.revision_id {
            let current_content = rpc_impl.convert_document_to_text(&current_document).await;

            let updated_doc = DocumentInfo {
                id: document_id.to_string(),
                title: current_document.title,
                revision_id: current_document.revision_id.clone(),
                content: current_content,
                suggestions_mode: cached_doc.suggestions_mode,
            };

            rpc_impl
                .document_cache
                .write()
                .await
                .insert(document_id.to_string(), updated_doc);

            let mut sync_statuses = rpc_impl.sync_statuses.write().await;
            if let Some(status) = sync_statuses.get_mut(document_id) {
                status.remote_revision = current_document.revision_id;
                status.last_sync = chrono::Utc::now();
            }

            return Ok(true);
        }

        Ok(false)
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            handle
                .stop()
                .map_err(|e| CedarError::JsonRpc(format!("Failed to stop server: {e}")))?;
            info!("JSON-RPC server stopped");
        }
        Ok(())
    }
}
