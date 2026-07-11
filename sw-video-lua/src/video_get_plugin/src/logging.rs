use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub(crate) fn append_log(path: &PathBuf, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating log dir {}: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("opening log {}: {error}", path.display()))?;
    let timestamp = log_timestamp();
    writeln!(file, "[{timestamp}] {line}")
        .map_err(|error| format!("writing log {}: {error}", path.display()))
}

pub(crate) fn clear_plugin_log_outputs(log_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(log_dir)
        .map_err(|error| format!("creating log dir {}: {error}", log_dir.display()))?;
    for name in [
        "video_get.log",
        "video_get_runtime_snapshot.json",
        "video_get_runtime_snapshots.jsonl",
        "video_get_runtime_heartbeat.json",
    ] {
        let path = log_dir.join(name);
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("clearing log {}: {error}", path.display()))?;
        }
    }
    for name in ["load_events", "frame_previews", "archive"] {
        let path = log_dir.join(name);
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("clearing log dir {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn log_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "timestamp_unavailable".to_string())
}

pub(crate) fn append_jsonl(path: &PathBuf, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating log dir {}: {error}", parent.display()))?;
    }
    let mut line =
        serde_json::to_vec(value).map_err(|error| format!("serializing jsonl: {error}"))?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("opening log {}: {error}", path.display()))?;
    file.write_all(&line)
        .and_then(|_| file.flush())
        .map_err(|error| format!("writing jsonl {}: {error}", path.display()))
}

pub(crate) fn write_json_pretty(path: &PathBuf, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating json dir {}: {error}", parent.display()))?;
    }
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| format!("serializing json: {error}"))?;
    fs::write(path, bytes).map_err(|error| format!("writing json {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_plugin_log_outputs_removes_previous_runtime_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "stormworks_video_get_clear_logs_test_{}_{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        fs::create_dir_all(root.join("load_events")).unwrap();
        fs::create_dir_all(root.join("frame_previews")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join("video_get.log"), "old").unwrap();
        fs::write(root.join("video_get_runtime_heartbeat.json"), "old").unwrap();
        fs::write(root.join("load_events").join("old.jsonl"), "old").unwrap();
        fs::write(root.join("frame_previews").join("old.bmp"), "old").unwrap();
        fs::write(root.join("archive").join("old.log"), "old").unwrap();

        clear_plugin_log_outputs(&root).unwrap();

        assert!(root.is_dir());
        assert!(!root.join("video_get.log").exists());
        assert!(!root.join("video_get_runtime_heartbeat.json").exists());
        assert!(!root.join("load_events").exists());
        assert!(!root.join("frame_previews").exists());
        assert!(!root.join("archive").exists());
        let _ = fs::remove_dir_all(root);
    }
}
