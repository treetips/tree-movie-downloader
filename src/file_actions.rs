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
    let path = expand_path(path);
    if !path.is_file() {
        return Err(format!("ファイルが見つかりません: {}", path.display()));
    }

    open_with_default_app(&path)
}

pub(crate) fn open_output_folder(path: &str) -> Result<(), String> {
    let path = expand_path(path);
    if !path.is_dir() {
        return Err(format!(
            "保存先フォルダが見つかりません: {}",
            path.display()
        ));
    }

    open_with_default_app(&path)
}

fn open_with_default_app(path: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}: 既定のアプリで開けませんでした（終了コード {}）",
            path.display(),
            status.code().unwrap_or(-1)
        ))
    }
}
