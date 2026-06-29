//! The ONE place that reads the environment. Everything else receives a
//! `&Config`. (Invariant: no `std::env::var` outside this module.)

use anyhow::{Context, Result};
use directories::ProjectDirs;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Resolved runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Absolute directory holding the SQLite database.
    pub data_dir: PathBuf,
    /// Editor command for interactive capture (`$VISUAL` / `$EDITOR`).
    #[serde(default)]
    pub editor: Option<String>,
}

impl Config {
    /// Load configuration once: built-in defaults, then an optional
    /// `note.toml` in the platform config dir, then `NOTE_*` env vars, then the
    /// CLI `--data-dir` override (highest priority).
    pub fn load(data_dir_override: Option<PathBuf>) -> Result<Self> {
        let defaults = Self {
            data_dir: default_data_dir()?,
            editor: None,
        };

        let mut fig = Figment::from(Serialized::defaults(defaults));
        if let Some(file) = config_file() {
            fig = fig.merge(Toml::file(file));
        }
        let mut cfg: Self = fig
            .merge(Env::prefixed("NOTE_"))
            .extract()
            .context("loading configuration")?;

        if let Some(dir) = data_dir_override {
            cfg.data_dir = dir;
        }
        if cfg.editor.is_none() {
            cfg.editor = env_nonempty("VISUAL").or_else(|| env_nonempty("EDITOR"));
        }

        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("creating data dir {}", cfg.data_dir.display()))?;
        Ok(cfg)
    }

    /// Path to the single SQLite database file.
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("notes.sqlite")
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "note")
}

fn default_data_dir() -> Result<PathBuf> {
    project_dirs()
        .map(|d| d.data_dir().to_owned())
        .context("could not determine a platform data directory")
}

fn config_file() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().join("note.toml"))
}
