//! ダウンロード済みファイルと保存先を開く処理。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

pub(crate) fn expand_path(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

pub(crate) fn open_output_file(path: &str) -> Result<(), String> {
    let path = expand_path(path);
    if !path.is_file() {
        return Err("ファイルが見つかりません".to_string());
    }

    open_with_default_app(&path, "ファイルを開けませんでした")
}

pub(crate) fn open_output_folder(path: &str) -> Result<(), String> {
    let path = expand_path(path);
    if !path.is_dir() {
        return Err("保存先フォルダが見つかりません".to_string());
    }

    open_with_default_app(&path, "保存先を開けませんでした")
}

fn open_with_default_app(path: &Path, error_prefix: &str) -> Result<(), String> {
    let mut child = Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map_err(|error| format!("{error_prefix}: {error}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
