//! Configuration — mirrors Python's config.py
//!
//! Load order: env vars > config file (~/.aimem/config.json) > defaults

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Default path for the Turso DB file (all data in one file).
pub fn default_db_path() -> PathBuf {
    dirs_next().join("palace.db")
}

/// Default path for the identity file.
pub fn default_identity_path() -> PathBuf {
    dirs_next().join("identity.txt")
}

fn dirs_next() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".aimem")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the Turso DB file.
    pub db_path: PathBuf,

    /// Path to the identity file (L0).
    pub identity_path: PathBuf,

    /// Drawer collection name (kept for reference; Turso uses a table).
    #[serde(default = "default_collection_name")]
    pub collection_name: String,

    /// Wings available by default.
    #[serde(default = "default_topic_wings")]
    pub topic_wings: Vec<String>,
}

fn default_collection_name() -> String {
    "aimem_drawers".to_string()
}

fn default_topic_wings() -> Vec<String> {
    vec![
        "emotions".into(),
        "consciousness".into(),
        "memory".into(),
        "technical".into(),
        "identity".into(),
        "family".into(),
        "creative".into(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            identity_path: default_identity_path(),
            collection_name: default_collection_name(),
            topic_wings: default_topic_wings(),
        }
    }
}

impl Config {
    /// Load config from env vars > `~/.aimem/config.json` > defaults.
    pub fn load() -> Result<Self, ConfigError> {
        let mut cfg = Self::default();

        // Override from config file
        let config_file = dirs_next().join("config.json");
        if config_file.exists() {
            let raw = std::fs::read_to_string(&config_file)?;
            let file_cfg: serde_json::Value = serde_json::from_str(&raw)?;

            if let Some(p) = file_cfg["db_path"].as_str() {
                cfg.db_path = PathBuf::from(shellexpand(p));
            }
            if let Some(p) = file_cfg["identity_path"].as_str() {
                cfg.identity_path = PathBuf::from(shellexpand(p));
            }
            if let Some(n) = file_cfg["collection_name"].as_str() {
                cfg.collection_name = n.to_string();
            }
        }

        // Override from env vars
        if let Ok(p) = std::env::var("AIMEM_DB_PATH").or(std::env::var("AIMEM_PALACE_PATH")) {
            // Legacy AIMEM_PALACE_PATH was a dir; new AIMEM_DB_PATH is a file
            let path = PathBuf::from(shellexpand(&p));
            if path.extension().is_some() {
                cfg.db_path = path;
            } else {
                cfg.db_path = path.join("palace.db");
            }
        }
        if let Ok(p) = std::env::var("AIMEM_IDENTITY_PATH") {
            cfg.identity_path = PathBuf::from(shellexpand(&p));
        }

        Ok(cfg)
    }

    /// Ensure the config directory exists and write a default config.json.
    pub fn init(&self) -> Result<(), ConfigError> {
        let dir = dirs_next();
        std::fs::create_dir_all(&dir)?;
        let config_file = dir.join("config.json");
        if !config_file.exists() {
            let json = serde_json::to_string_pretty(self)?;
            std::fs::write(&config_file, json)?;
        }
        Ok(())
    }
}

/// Simple `~` expansion (no full shell expansion needed).
fn shellexpand(s: &str) -> String {
    if s.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/{}", home, &s[2..])
    } else {
        s.to_string()
    }
}
