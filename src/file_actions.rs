//! ダウンロード済みファイルと保存先を開く処理。

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let path = Path::new(path);
    if !path.is_file() {
        return Err("ファイルが見つかりません".to_string());
    }

    Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("ファイルを開けませんでした: {error}"))
}

pub(crate) fn open_output_folder(path: &str) -> Result<(), String> {
    let path = expand_path(path);
    if !path.is_dir() {
        return Err("保存先フォルダが見つかりません".to_string());
    }

    Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("保存先を開けませんでした: {error}"))
}
