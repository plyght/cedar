use crate::config::GoogleConfig;
use crate::errors::{CedarError, Result};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, HttpRequest,
    HttpResponse, PkceCodeChallenge, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::fs;
use tracing::{debug, info, warn};
use url::Url;

#[derive(Debug, thiserror::Error)]
enum OAuth2HttpError {
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Invalid HTTP status code: {0}")]
    InvalidStatusCode(#[from] http::status::InvalidStatusCode),
    #[error("Invalid header name: {0}")]
    InvalidHeaderName(#[from] http::header::InvalidHeaderName),
    #[error("Invalid header value: {0}")]
    InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct AuthManager {
    client: BasicClient,
    config: GoogleConfig,
    http_client: Client,
}

impl AuthManager {
    pub fn new(config: GoogleConfig) -> Result<Self> {
        let client_id = config
            .client_id
            .as_ref()
            .ok_or_else(|| CedarError::Auth("Client ID not configured".to_string()))?;

        let client_secret = config
            .client_secret
            .as_ref()
            .ok_or_else(|| CedarError::Auth("Client secret not configured".to_string()))?;

        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .map_err(|e| CedarError::Auth(format!("Invalid auth URL: {e}")))?;

        let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
            .map_err(|e| CedarError::Auth(format!("Invalid token URL: {e}")))?;

        let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
            .map_err(|e| CedarError::Auth(format!("Invalid redirect URI: {e}")))?;

        let client = BasicClient::new(
            ClientId::new(client_id.clone()),
            Some(ClientSecret::new(client_secret.clone())),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(redirect_url);

        Ok(Self {
            client,
            config,
            http_client: Client::new(),
        })
    }

    pub async fn get_valid_credentials(&mut self) -> Result<Credentials> {
        if let Ok(credentials) = self.load_credentials().await {
            if !self.is_token_expired(&credentials) {
                debug!("Using cached access token");
                return Ok(credentials);
            }

            if let Some(refresh_token) = &credentials.refresh_token {
                info!("Access token expired, attempting refresh");
                if let Ok(refreshed) = self.refresh_access_token(refresh_token).await {
                    return Ok(refreshed);
                }
                warn!("Token refresh failed, initiating new authentication flow");
            }
        }

        info!("Starting OAuth2 authentication flow");
        self.authenticate().await
    }

    pub async fn authenticate(&self) -> Result<Credentials> {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut auth_request = self.client.authorize_url(CsrfToken::new_random);

        for scope in &self.config.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        let (auth_url, csrf_token) = auth_request.set_pkce_challenge(pkce_challenge).url();

        println!("Please visit this URL to authorize Cedar:");
        println!("{auth_url}");
        println!("\nWaiting for authorization callback...");

        let authorization_code = self.start_callback_server(csrf_token).await?;

        let http_client = &self.http_client;
        let token_result = self
            .client
            .exchange_code(authorization_code)
            .set_pkce_verifier(pkce_verifier)
            .request_async(|request| async move {
                Self::execute_http_request(http_client, request).await
            })
            .await
            .map_err(|e| CedarError::Auth(format!("Token exchange failed: {e}")))?;

        let credentials = Credentials {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_at: token_result
                .expires_in()
                .map(|duration| chrono::Utc::now() + chrono::Duration::from_std(duration).unwrap()),
        };

        self.save_credentials(&credentials).await?;
        info!("Authentication successful");

        Ok(credentials)
    }

    async fn start_callback_server(&self, expected_csrf: CsrfToken) -> Result<AuthorizationCode> {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        let result = Arc::new(Mutex::new(None));

        let (tx, rx) = oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));

        let redirect_url = Url::parse(&self.config.redirect_uri)
            .map_err(|e| CedarError::Auth(format!("Invalid redirect URI: {e}")))?;

        let port = redirect_url
            .port()
            .ok_or_else(|| CedarError::Auth("Redirect URI must include a port".to_string()))?;

        let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .map_err(|e| CedarError::Auth(format!("Failed to bind callback server: {e}")))?;

        let result_clone = Arc::clone(&result);
        let tx_clone = Arc::clone(&tx);

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0; 1024];
                if let Ok(bytes_read) = stream.read(&mut buffer).await {
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                    if let Some(line) = request.lines().next() {
                        if let Some(path) = line.split_whitespace().nth(1) {
                            if let Some(query_start) = path.find('?') {
                                let query = &path[query_start + 1..];

                                let response = match Self::parse_callback_query(
                                    query,
                                    expected_csrf,
                                ) {
                                    Ok(code) => {
                                        *result_clone.lock().unwrap() = Some(Ok(code));
                                        "HTTP/1.1 200 OK\r\n\r\nAuthorization successful! You can close this window."
                                    }
                                    Err(e) => {
                                        *result_clone.lock().unwrap() = Some(Err(e));
                                        "HTTP/1.1 400 Bad Request\r\n\r\nAuthorization failed"
                                    }
                                };

                                let _ = stream.write_all(response.as_bytes()).await;
                            }
                        }
                    }
                }

                if let Some(sender) = tx_clone.lock().unwrap().take() {
                    let _ = sender.send(());
                }
            }
        });

        match rx.await {
            Ok(_) => match result.lock().unwrap().take() {
                Some(Ok(code)) => Ok(code),
                Some(Err(e)) => Err(e),
                None => Err(CedarError::Auth(
                    "No authorization result received".to_string(),
                )),
            },
            Err(_) => Err(CedarError::Auth("Callback server task failed".to_string())),
        }
    }

    async fn execute_http_request(
        client: &Client,
        request: HttpRequest,
    ) -> std::result::Result<HttpResponse, OAuth2HttpError> {
        let method_str = request.method.to_string();
        let mut req_builder = client.request(
            method_str.parse().unwrap_or(reqwest::Method::GET),
            request.url.to_string(),
        );

        for (name, value) in &request.headers {
            let header_name = name.to_string();
            let header_value = std::str::from_utf8(value.as_bytes())
                .unwrap_or("")
                .to_string();
            req_builder = req_builder.header(header_name, header_value);
        }

        if !request.body.is_empty() {
            req_builder = req_builder.body(request.body.clone());
        }

        let response = req_builder.send().await?;
        let status_code = response.status().as_u16();
        let headers_map = response.headers().clone();
        let body = response.bytes().await?;

        // Convert reqwest types to oauth2 types
        use oauth2::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};

        let oauth_status = StatusCode::from_u16(status_code)?;
        let mut oauth_headers = HeaderMap::new();

        for (name, value) in headers_map {
            if let (Some(name), Ok(value_str)) = (name, std::str::from_utf8(value.as_bytes())) {
                if let (Ok(header_name), Ok(header_value)) = (
                    HeaderName::from_bytes(name.as_str().as_bytes()),
                    HeaderValue::from_str(value_str),
                ) {
                    oauth_headers.insert(header_name, header_value);
                }
            }
        }

        Ok(HttpResponse {
            status_code: oauth_status,
            headers: oauth_headers,
            body: body.to_vec(),
        })
    }

    fn parse_callback_query(query: &str, expected_csrf: CsrfToken) -> Result<AuthorizationCode> {
        let params: HashMap<String, String> = query
            .split('&')
            .filter_map(|param| {
                let mut parts = param.splitn(2, '=');
                match (parts.next(), parts.next()) {
                    (Some(key), Some(value)) => Some((
                        key.to_string(),
                        urlencoding::decode(value).unwrap().to_string(),
                    )),
                    _ => None,
                }
            })
            .collect();

        if let Some(error) = params.get("error") {
            return Err(CedarError::Auth(format!("OAuth error: {error}")));
        }

        let state = params
            .get("state")
            .ok_or_else(|| CedarError::Auth("Missing state parameter".to_string()))?;

        if state != expected_csrf.secret() {
            return Err(CedarError::Auth("CSRF token mismatch".to_string()));
        }

        let code = params
            .get("code")
            .ok_or_else(|| CedarError::Auth("Missing authorization code".to_string()))?;

        Ok(AuthorizationCode::new(code.clone()))
    }

    async fn refresh_access_token(&self, refresh_token: &str) -> Result<Credentials> {
        let grant_type = "refresh_token".to_string();
        let params = [
            (
                "client_id",
                self.config.client_id.as_ref().unwrap().as_str(),
            ),
            (
                "client_secret",
                self.config.client_secret.as_ref().unwrap().as_str(),
            ),
            ("refresh_token", refresh_token),
            ("grant_type", grant_type.as_str()),
        ];

        let response = self
            .http_client
            .post("https://www.googleapis.com/oauth2/v3/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| CedarError::Auth(format!("Token refresh request failed: {e}")))?;

        let token_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CedarError::Auth(format!("Failed to parse token response: {e}")))?;

        if let Some(error) = token_response.get("error") {
            return Err(CedarError::Auth(format!("Token refresh failed: {error}")));
        }

        let access_token = token_response["access_token"]
            .as_str()
            .ok_or_else(|| CedarError::Auth("Missing access token in response".to_string()))?
            .to_string();

        let expires_in = token_response
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .map(|seconds| chrono::Utc::now() + chrono::Duration::seconds(seconds as i64));

        let credentials = Credentials {
            access_token,
            refresh_token: Some(refresh_token.to_string()),
            expires_at: expires_in,
        };

        self.save_credentials(&credentials).await?;
        info!("Access token refreshed successfully");

        Ok(credentials)
    }

    fn is_token_expired(&self, credentials: &Credentials) -> bool {
        credentials.expires_at.is_some_and(|exp| {
            chrono::Utc::now() + chrono::Duration::minutes(5) > exp
        })
    }

    async fn load_credentials(&self) -> Result<Credentials> {
        if !self.config.credentials_file.exists() {
            return Err(CedarError::Auth("No credentials file found".to_string()));
        }

        let contents = fs::read_to_string(&self.config.credentials_file)
            .await
            .map_err(|e| CedarError::Auth(format!("Failed to read credentials file: {e}")))?;

        serde_json::from_str(&contents)
            .map_err(|e| CedarError::Auth(format!("Failed to parse credentials file: {e}")))
    }

    async fn save_credentials(&self, credentials: &Credentials) -> Result<()> {
        if let Some(parent) = self.config.credentials_file.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                CedarError::Auth(format!("Failed to create credentials directory: {e}"))
            })?;
        }

        let contents = serde_json::to_string_pretty(credentials)
            .map_err(|e| CedarError::Auth(format!("Failed to serialize credentials: {e}")))?;

        fs::write(&self.config.credentials_file, contents)
            .await
            .map_err(|e| CedarError::Auth(format!("Failed to write credentials file: {e}")))?;

        info!("Credentials saved to {:?}", self.config.credentials_file);
        Ok(())
    }
}
