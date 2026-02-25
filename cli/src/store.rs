use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{
    AppSettings, EngineState, RunSnapshot, default_fix_template, default_review_template,
};

pub struct StorePaths {
    pub root: PathBuf,
    pub settings: PathBuf,
    pub state: PathBuf,
    pub snapshot: PathBuf,
    pub reports: PathBuf,
    pub logs: PathBuf,
}

impl StorePaths {
    pub fn new() -> Result<Self> {
        let root = if let Ok(path) = std::env::var("PR_REVIEWER_HOME") {
            PathBuf::from(path)
        } else {
            let home = dirs::home_dir().context("cannot resolve home directory")?;
            home.join(".pr-reviewer-cli")
        };

        let paths = Self {
            settings: root.join("settings.json"),
            state: root.join("engine-state.json"),
            snapshot: root.join("run-snapshot.json"),
            reports: root.join("reports"),
            logs: root.join("logs"),
            root,
        };

        fs::create_dir_all(&paths.root)?;
        fs::create_dir_all(&paths.reports)?;
        fs::create_dir_all(&paths.logs)?;
        Ok(paths)
    }
}

pub fn load_json_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read file: {}", path.display()))?;
    let value = serde_json::from_str::<T>(&content)
        .with_context(|| format!("failed to parse json: {}", path.display()))?;
    Ok(value)
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content)
        .with_context(|| format!("failed to write file: {}", path.display()))?;
    Ok(())
}

pub fn load_settings(paths: &StorePaths) -> Result<AppSettings> {
    if !paths.settings.exists() {
        let defaults = AppSettings::default();
        save_json(&paths.settings, &defaults)?;
        return Ok(defaults);
    }

    let mut settings: AppSettings = load_json_or_default(&paths.settings)?;
    let mut migrated = false;

    if settings
        .review_command_template
        .contains("codex review --pr")
        || settings
            .review_command_template
            .contains("--repo {{REPO_PATH}}")
        || (settings
            .review_command_template
            .contains("codex review --base")
            && (settings.review_command_template.contains("{{PR_")
                || settings.review_command_template.contains("\"Review ")))
    {
        settings.review_command_template = default_review_template();
        migrated = true;
    }

    if settings
        .fix_command_template
        .trim_start()
        .starts_with("codex fix")
    {
        settings.fix_command_template = default_fix_template();
        migrated = true;
    }

    if migrated {
        save_json(&paths.settings, &settings)?;
    }

    Ok(settings)
}

pub fn load_engine_state(paths: &StorePaths) -> Result<EngineState> {
    load_json_or_default(&paths.state)
}

pub fn save_engine_state(paths: &StorePaths, state: &EngineState) -> Result<()> {
    save_json(&paths.state, state)
}

pub fn load_snapshot(paths: &StorePaths) -> Result<RunSnapshot> {
    load_json_or_default(&paths.snapshot)
}

pub fn save_snapshot(paths: &StorePaths, snapshot: &RunSnapshot) -> Result<()> {
    save_json(&paths.snapshot, snapshot)
}
