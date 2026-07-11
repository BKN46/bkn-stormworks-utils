use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_GAME_DIR: &str = "D:/SteamLibrary/steamapps/common/Stormworks";
pub const GAME_EXE_NAME: &str = "stormworks64.exe";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerConfig {
    pub game: GameConfig,
    pub launch: LaunchConfig,
    pub plugins: serde_json::Value,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub install_path: PathBuf,
    pub auto_detect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub allow_attach: bool,
    pub single_player_only: bool,
    pub fail_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub open_log_on_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub entry: PluginEntry,
    pub supported_process: String,
    pub architecture: String,
    pub game_builds: Vec<GameBuild>,
    pub default_enabled: bool,
    pub single_player_only: bool,
    pub docs: Option<String>,
    pub config_schema: Option<String>,
    pub default_config: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameBuild {
    pub label: String,
    pub sha256: String,
    pub signatures: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRuntimeContext {
    pub schema_version: u32,
    pub manager_home: PathBuf,
    pub plugin_id: String,
    pub plugin_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub signatures_path: PathBuf,
    #[serde(default)]
    pub hook_plan_path: Option<PathBuf>,
    pub game_exe: PathBuf,
    pub game_sha256: String,
    pub game_build_label: String,
    pub mode: String,
    pub process_id: Option<u32>,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    Disabled,
    Ready,
    MissingFiles(String),
    UnsupportedGameVersion,
    BlockedByPolicy(String),
    InvalidManifest(String),
}

impl PluginState {
    pub fn as_str(&self) -> &str {
        match self {
            PluginState::Disabled => "disabled",
            PluginState::Ready => "ready",
            PluginState::MissingFiles(_) => "missing_files",
            PluginState::UnsupportedGameVersion => "unsupported_game_version",
            PluginState::BlockedByPolicy(_) => "blocked_by_policy",
            PluginState::InvalidManifest(_) => "invalid_manifest",
        }
    }
}

pub fn default_manager_config() -> ManagerConfig {
    ManagerConfig {
        game: GameConfig {
            install_path: PathBuf::from(DEFAULT_GAME_DIR),
            auto_detect: true,
        },
        launch: LaunchConfig {
            allow_attach: true,
            single_player_only: true,
            fail_closed: true,
        },
        plugins: serde_json::json!({
            "video_get": {
                "enabled": true
            }
        }),
        logging: LoggingConfig {
            level: "info".to_string(),
            open_log_on_failure: true,
        },
    }
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).with_context(|| format!("parsing json {}", path.display()))
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn game_exe_path(game_dir: &Path) -> PathBuf {
    game_dir.join(GAME_EXE_NAME)
}

pub fn validate_plugin(
    plugin_dir: &Path,
    manifest: &PluginManifest,
    game_sha256: Option<&str>,
    enabled: bool,
) -> PluginState {
    if !enabled {
        return PluginState::Disabled;
    }

    if manifest.supported_process != GAME_EXE_NAME {
        return PluginState::BlockedByPolicy(format!(
            "unsupported process {}",
            manifest.supported_process
        ));
    }

    if manifest.architecture.to_ascii_lowercase() != "x64" {
        return PluginState::BlockedByPolicy(format!(
            "unsupported architecture {}",
            manifest.architecture
        ));
    }

    let entry_path = plugin_dir.join(&manifest.entry.path);
    if !entry_path.exists() {
        return PluginState::MissingFiles(entry_path.display().to_string());
    }

    let Some(game_sha256) = game_sha256 else {
        return PluginState::UnsupportedGameVersion;
    };

    let Some(build) = manifest
        .game_builds
        .iter()
        .find(|build| build.sha256.eq_ignore_ascii_case(game_sha256))
    else {
        return PluginState::UnsupportedGameVersion;
    };

    let signatures_path = plugin_dir.join(&build.signatures);
    if !signatures_path.exists() {
        return PluginState::MissingFiles(signatures_path.display().to_string());
    }

    PluginState::Ready
}

pub fn matching_game_build<'a>(
    manifest: &'a PluginManifest,
    game_sha256: &str,
) -> Option<&'a GameBuild> {
    manifest
        .game_builds
        .iter()
        .find(|build| build.sha256.eq_ignore_ascii_case(game_sha256))
}

pub fn plugin_enabled(config: &ManagerConfig, plugin_id: &str, default_enabled: bool) -> bool {
    config
        .plugins
        .get(plugin_id)
        .and_then(|value| value.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(default_enabled)
}

pub fn discover_plugin_dirs(plugins_root: &Path) -> Result<Vec<PathBuf>> {
    if !plugins_root.exists() {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(plugins_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join("plugin.json").exists() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}
