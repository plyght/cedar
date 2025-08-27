use crate::errors::{CedarError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub google: GoogleConfig,
    pub server: ServerConfig,
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub credentials_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub poll_interval_seconds: u64,
    pub debounce_milliseconds: u64,
    pub max_retries: u32,
    pub backoff_multiplier: f64,
}

impl Default for Config {
    fn default() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cedar");

        Self {
            google: GoogleConfig {
                client_id: None,
                client_secret: None,
                redirect_uri: "http://127.0.0.1:8080".to_string(),
                scopes: vec![
                    "https://www.googleapis.com/auth/documents".to_string(),
                    "https://www.googleapis.com/auth/drive.readonly".to_string(),
                ],
                credentials_file: config_dir.join("credentials.json"),
            },
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3030,
            },
            sync: SyncConfig {
                poll_interval_seconds: 5,
                debounce_milliseconds: 500,
                max_retries: 3,
                backoff_multiplier: 2.0,
            },
        }
    }
}

impl Config {
    pub async fn load() -> Result<Self> {
        let config_path = Self::config_file_path();
        
        if config_path.exists() {
            info!("Loading config from {:?}", config_path);
            let contents = fs::read_to_string(&config_path)
                .await
                .map_err(|e| CedarError::Config(format!("Failed to read config file: {}", e)))?;
            
            let config: Config = toml::from_str(&contents)
                .map_err(|e| CedarError::Config(format!("Failed to parse config file: {}", e)))?;
            
            Ok(config)
        } else {
            warn!("Config file not found, using defaults");
            let config = Config::default();
            config.save().await?;
            Ok(config)
        }
    }

    pub async fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path();
        
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| CedarError::Config(format!("Failed to create config directory: {}", e)))?;
        }

        let contents = toml::to_string_pretty(self)
            .map_err(|e| CedarError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(&config_path, contents)
            .await
            .map_err(|e| CedarError::Config(format!("Failed to write config file: {}", e)))?;

        info!("Config saved to {:?}", config_path);
        Ok(())
    }

    fn config_file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cedar")
            .join("config.toml")
    }
}