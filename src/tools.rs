//! yt-dlp・ffmpeg・Deno の配布バイナリを GitHub Releases から取得し、設定ディレクトリへ配置する。
//!
//! アプリは外部ツールを同梱せず、起動時に不足しているバイナリをダウンロードする。
//! 配布物の一覧は `tools-info.json` で管理し、取得した ZIP は SHA-256 で検証する。
//! 更新確認で選択されたツールだけを差し替えることもできる。
//! 呼び出し側は専用スレッドで実行し、進捗はイベントとして UI へ通知する。

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_settings::config_dir;

/// 配布物の一覧を置く GitHub Releases の URL。
///
/// アプリ本体のリリースとは独立して更新できるよう、`tools-latest` タグへ集約する。
pub const MANIFEST_URL: &str = "https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/tools-info.json";

/// `tools-info.json` のスキーマ。形式を変えた場合はアプリ側の対応も必要になる。
const SCHEMA_VERSION: u32 = 1;

/// ダウンロードの最大再試行回数（初回を除く）。
const MAX_DOWNLOAD_RETRIES: u32 = 3;

/// yt-dlp のツール名。`tools-info.json` の `name` と一致させる。
pub const YT_DLP: &str = "yt-dlp";
/// ffmpeg のツール名。`tools-info.json` の `name` と一致させる。
pub const FFMPEG: &str = "ffmpeg";
/// Deno のツール名。`tools-info.json` の `name` と一致させる。
pub const DENO: &str = "deno";

/// 起動時に必要なツール。配布情報にも同じものを必須として含める。
const REQUIRED_TOOLS: [&str; 3] = [YT_DLP, FFMPEG, DENO];

/// バイナリを保存するディレクトリを上書きする環境変数。開発・テスト用。
const TOOLS_DIR_ENV: &str = "TMD_TOOLS_DIR";
/// 配布物一覧の URL を上書きする環境変数。開発用。
const MANIFEST_URL_ENV: &str = "TMD_TOOLS_INFO_URL";

/// 現在の OS と CPU。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    os: &'static str,
    arch: &'static str,
}

impl Platform {
    /// 実行中のプラットフォーム。配布対象外なら `None`。
    pub fn current() -> Option<Self> {
        let os = match std::env::consts::OS {
            "macos" => "macos",
            "windows" => "windows",
            "linux" => "linux",
            _ => return None,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" | "arm64" => "aarch64",
            "x86_64" | "x86-64" => "x86_64",
            _ => return None,
        };
        Some(Self { os, arch })
    }

    /// `tools-info.json` の `platform` と一致する識別子。
    pub fn id(self) -> String {
        format!("{}-{}", self.os, self.arch)
    }
}

/// `tools-info.json` 全体。
#[derive(Clone, Debug, Deserialize)]
pub struct ToolsInfo {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub tools: Vec<ToolEntry>,
}

/// 1 プラットフォーム分のツール配布物。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ToolEntry {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub required: bool,
}

/// ローカルにインストール済みのツール記録。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct InstalledTools {
    #[serde(default, rename = "schemaVersion")]
    schema_version: u32,
    #[serde(default)]
    tools: BTreeMap<String, InstalledTool>,
}

/// ツール 1 件分のインストール記録。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstalledTool {
    pub version: String,
    pub platform: String,
}

impl InstalledTools {
    fn get(&self, name: &str) -> Option<&InstalledTool> {
        self.tools.get(name)
    }

    fn set(&mut self, name: &str, tool: InstalledTool) {
        self.schema_version = SCHEMA_VERSION;
        self.tools.insert(name.to_string(), tool);
    }
}

/// ツールの配置先。
#[derive(Clone, Debug)]
pub struct ToolPaths {
    root: PathBuf,
}

impl ToolPaths {
    /// 設定ディレクトリ配下の既定の配置先。`TMD_TOOLS_DIR` があればそちらを使う。
    pub fn from_env() -> Self {
        let root = std::env::var_os(TOOLS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir().join("tools"));
        Self { root }
    }

    /// 任意のディレクトリを使う配置先。テスト用。
    #[cfg(test)]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tool_dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn binary(&self, tool: &str, file: &str) -> PathBuf {
        self.tool_dir(tool).join(executable_name(file))
    }

    /// 管理下の yt-dlp。
    pub fn yt_dlp(&self) -> PathBuf {
        self.binary(YT_DLP, "yt-dlp")
    }

    /// 管理下の ffmpeg。
    pub fn ffmpeg(&self) -> PathBuf {
        self.binary(FFMPEG, "ffmpeg")
    }

    /// 管理下の ffprobe。
    pub fn ffprobe(&self) -> PathBuf {
        self.binary(FFMPEG, "ffprobe")
    }

    /// yt-dlp に `--ffmpeg-location` として渡すディレクトリ。
    pub fn ffmpeg_dir(&self) -> PathBuf {
        self.tool_dir(FFMPEG)
    }

    /// 管理下の Deno。
    pub fn deno(&self) -> PathBuf {
        self.binary(DENO, "deno")
    }

    /// 必須バイナリがすべて実行可能な状態で存在するか。バージョンの一致までは見ない。
    pub fn is_complete(&self) -> bool {
        is_executable_file(&self.yt_dlp())
            && is_executable_file(&self.ffmpeg())
            && is_executable_file(&self.ffprobe())
            && is_executable_file(&self.deno())
    }

    fn is_tool_complete(&self, name: &str) -> bool {
        expected_files(name)
            .iter()
            .all(|file| is_executable_file(&self.binary(name, file)))
    }

    fn installed_path(&self) -> PathBuf {
        self.root.join(".installed.json")
    }

    fn load_installed(&self) -> InstalledTools {
        fs::read_to_string(self.installed_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_installed(&self, installed: &InstalledTools) -> Result<(), String> {
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("ツールの保存先を作成できません: {error}"))?;
        let json = serde_json::to_string_pretty(installed)
            .map_err(|error| format!("ツールの状態を保存できません: {error}"))?;
        let temporary = self
            .root
            .join(format!(".installed-{}.tmp", std::process::id()));
        let result = (|| {
            fs::write(&temporary, json)?;
            fs::rename(&temporary, self.installed_path())
        })();
        let _ = fs::remove_file(&temporary);
        result.map_err(|error| format!("ツールの状態を保存できません: {error}"))
    }

    /// `.installed.json` に記録されたバージョン。
    pub fn installed_version(&self, name: &str) -> Option<String> {
        self.load_installed()
            .get(name)
            .map(|tool| tool.version.clone())
    }
}

/// 起動時の準備状況。UI はこのイベントだけを見て表示を切り替える。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapEvent {
    /// 配布物一覧を取得している。
    Checking,
    /// ツールをダウンロードしている。
    Downloading {
        tool: String,
        received: u64,
        total: Option<u64>,
        step: usize,
        steps: usize,
    },
    /// ダウンロードしたツールを展開・配置している。
    Installing {
        tool: String,
        step: usize,
        steps: usize,
    },
    /// すべてのツールが揃った。
    Ready,
    /// 準備に失敗した。UI は再試行できる。
    Failed(String),
}

/// 不足しているツールを取得する。呼び出し側は専用スレッドで実行する。
pub fn bootstrap(paths: ToolPaths, events: smol::channel::Sender<BootstrapEvent>) {
    if paths.is_complete() {
        let _ = events.send_blocking(BootstrapEvent::Ready);
        return;
    }

    let _ = events.send_blocking(BootstrapEvent::Checking);
    match bootstrap_with_url(&paths, &manifest_url(), &events) {
        Ok(()) => {
            let _ = events.send_blocking(BootstrapEvent::Ready);
        }
        Err(message) => {
            let _ = events.send_blocking(BootstrapEvent::Failed(message));
        }
    }
}

fn manifest_url() -> String {
    std::env::var(MANIFEST_URL_ENV).unwrap_or_else(|_| MANIFEST_URL.to_string())
}

/// 配布情報を確認し、更新が必要なツールだけを返す。呼び出し側は専用スレッドで実行する。
pub fn check_for_tool_updates(paths: ToolPaths) -> Result<Vec<ToolEntry>, String> {
    check_for_tool_updates_with_url(&paths, &manifest_url())
}

fn check_for_tool_updates_with_url(paths: &ToolPaths, url: &str) -> Result<Vec<ToolEntry>, String> {
    let platform = current_platform()?;
    let info = fetch_manifest(url)?;
    let entries = required_entries(&info, platform)?;
    let installed = paths.load_installed();
    Ok(pending_entries(paths, &installed, &entries)
        .into_iter()
        .cloned()
        .collect())
}

/// 確認済みのツールだけを更新する。呼び出し側は専用スレッドで実行する。
pub fn install_tool_updates(
    paths: ToolPaths,
    entries: Vec<ToolEntry>,
    events: smol::channel::Sender<BootstrapEvent>,
) {
    let result = install_entries(&paths, &entries, &events);
    let event = match result {
        Ok(()) => BootstrapEvent::Ready,
        Err(message) => BootstrapEvent::Failed(message),
    };
    let _ = events.send_blocking(event);
}

fn current_platform() -> Result<Platform, String> {
    Platform::current().ok_or_else(|| {
        format!(
            "この環境（{} / {}）向けのツール配布はありません。",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn bootstrap_with_url(
    paths: &ToolPaths,
    url: &str,
    events: &smol::channel::Sender<BootstrapEvent>,
) -> Result<(), String> {
    let platform = current_platform()?;
    let info = fetch_manifest(url)?;
    let entries = required_entries(&info, platform)?;
    let installed = paths.load_installed();
    let pending: Vec<ToolEntry> = pending_entries(paths, &installed, &entries)
        .into_iter()
        .cloned()
        .collect();
    install_entries(paths, &pending, events)
}

fn pending_entries<'a>(
    paths: &ToolPaths,
    installed: &InstalledTools,
    entries: &'a [ToolEntry],
) -> Vec<&'a ToolEntry> {
    entries
        .iter()
        .filter(|entry| {
            let is_current = installed.get(&entry.name).is_some_and(|tool| {
                tool.version == entry.version && tool.platform == entry.platform
            });
            !(is_current && paths.is_tool_complete(&entry.name))
        })
        .collect()
}

fn install_entries(
    paths: &ToolPaths,
    entries: &[ToolEntry],
    events: &smol::channel::Sender<BootstrapEvent>,
) -> Result<(), String> {
    fs::create_dir_all(paths.root())
        .map_err(|error| format!("ツールの保存先を作成できません: {error}"))?;
    cleanup_temporary_files(paths.root());

    let mut installed = paths.load_installed();
    let steps = entries.len();
    for (index, entry) in entries.iter().enumerate() {
        let step = index + 1;
        install_entry(paths, entry, events, step, steps)?;
        installed.set(
            &entry.name,
            InstalledTool {
                version: entry.version.clone(),
                platform: entry.platform.clone(),
            },
        );
        paths.save_installed(&installed)?;
    }

    if !paths.is_complete() {
        return Err("ツールの配置が完了しませんでした。再試行してください。".to_string());
    }
    Ok(())
}

fn fetch_manifest(url: &str) -> Result<ToolsInfo, String> {
    let agent = agent(Duration::from_secs(10));
    let mut response = match agent.get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!(
                "ツールの配布情報が HTTP {code} を返しました（{url}）。配布物が公開されているか確認してください。"
            ));
        }
        Err(error) => {
            return Err(format!(
                "ツールの配布情報を取得できませんでした（{url}）: {error}。ネットワーク接続を確認してください。"
            ));
        }
    };
    let text = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|error| format!("ツールの配布情報を読み取れませんでした: {error}"))?;
    let info: ToolsInfo = serde_json::from_str(&text)
        .map_err(|error| format!("ツールの配布情報の形式が正しくありません: {error}"))?;
    if info.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "ツールの配布情報のスキーマ（{}）に対応していません。アプリを更新してください。",
            info.schema_version
        ));
    }
    Ok(info)
}

fn required_entries(info: &ToolsInfo, platform: Platform) -> Result<Vec<ToolEntry>, String> {
    let platform_id = platform.id();
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for entry in info
        .tools
        .iter()
        .filter(|entry| entry.required && entry.platform == platform_id)
    {
        // このアプリがまだ知らないツールは無視する。配布情報へ新しいツールを
        // 先に追加しても、古いアプリの起動を妨げないようにするため。
        if expected_files(&entry.name).is_empty() {
            continue;
        }
        if !seen.insert(entry.name.clone()) {
            return Err(format!(
                "ツールの配布情報に {} が重複しています。",
                entry.name
            ));
        }
        if entry.sha256.len() != 64 || !entry.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!(
                "ツール {} の配布情報に正しいチェックサムがありません。",
                entry.name
            ));
        }
        if !entry.url.starts_with("https://") {
            return Err(format!(
                "ツール {} の配布元が https ではありません。",
                entry.name
            ));
        }
        entries.push(entry.clone());
    }

    for name in REQUIRED_TOOLS {
        if !entries.iter().any(|entry| entry.name == name) {
            return Err(format!(
                "この環境（{platform_id}）向けの {name} が配布されていません。"
            ));
        }
    }
    entries.sort_by_key(|entry| match entry.name.as_str() {
        YT_DLP => 0,
        FFMPEG => 1,
        DENO => 2,
        _ => 3,
    });
    Ok(entries)
}

fn install_entry(
    paths: &ToolPaths,
    entry: &ToolEntry,
    events: &smol::channel::Sender<BootstrapEvent>,
    step: usize,
    steps: usize,
) -> Result<(), String> {
    let archive = paths.root().join(format!(
        ".download-{}-{}.zip",
        entry.name,
        std::process::id()
    ));
    let staging = paths
        .root()
        .join(format!(".staging-{}-{}", entry.name, std::process::id()));
    let backup = paths
        .root()
        .join(format!(".backup-{}-{}", entry.name, std::process::id()));
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&staging);
    remove_path(&backup);

    let result = (|| {
        let actual = download_entry(entry, &archive, events, step, steps)?;
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!(
                "{} のチェックサムが一致しません。配布物が壊れている可能性があります。",
                entry.name
            ));
        }

        let _ = events.send_blocking(BootstrapEvent::Installing {
            tool: entry.name.clone(),
            step,
            steps,
        });
        fs::create_dir_all(&staging)
            .map_err(|error| format!("{} の展開先を作成できません: {error}", entry.name))?;
        extract_expected_files(&archive, &staging, &entry.name)?;

        let destination = paths.tool_dir(&entry.name);
        let had_destination = destination.exists();
        if had_destination {
            fs::rename(&destination, &backup).map_err(|error| {
                format!("古い {} をバックアップできません: {error}", entry.name)
            })?;
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            if had_destination {
                fs::rename(&backup, &destination).map_err(|restore_error| {
                    format!(
                        "{} を配置できません: {error}。古いツールも復元できませんでした: {restore_error}",
                        entry.name
                    )
                })?;
            }
            return Err(format!("{} を配置できません: {error}", entry.name));
        }
        remove_path(&backup);
        Ok(())
    })();

    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn download_entry(
    entry: &ToolEntry,
    destination: &Path,
    events: &smol::channel::Sender<BootstrapEvent>,
    step: usize,
    steps: usize,
) -> Result<String, String> {
    let agent = agent(Duration::from_secs(30 * 60));
    let mut last_error = String::new();

    for attempt in 0..=MAX_DOWNLOAD_RETRIES {
        let _ = fs::remove_file(destination);
        let _ = events.send_blocking(BootstrapEvent::Downloading {
            tool: entry.name.clone(),
            received: 0,
            total: None,
            step,
            steps,
        });

        match download_once(
            &agent,
            &entry.url,
            destination,
            &entry.name,
            events,
            step,
            steps,
        ) {
            Ok(hash) => return Ok(hash),
            Err(DownloadError::Permanent(message)) => return Err(message),
            Err(DownloadError::Retryable(message)) => {
                last_error = message;
                if attempt < MAX_DOWNLOAD_RETRIES {
                    std::thread::sleep(Duration::from_secs(1 << attempt));
                }
            }
        }
    }

    Err(format!(
        "{} をダウンロードできませんでした（{MAX_DOWNLOAD_RETRIES}回再試行）: {last_error}",
        entry.name
    ))
}

fn download_once(
    agent: &ureq::Agent,
    url: &str,
    destination: &Path,
    tool: &str,
    events: &smol::channel::Sender<BootstrapEvent>,
    step: usize,
    steps: usize,
) -> Result<String, DownloadError> {
    let response = agent
        .get(url)
        .call()
        .map_err(|error| classify_download_error(url, error))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(destination).map_err(|error| {
        DownloadError::Permanent(format!("{tool} の保存ファイルを作成できません: {error}"))
    })?;

    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    let mut last_update = Instant::now();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            DownloadError::Retryable(format!("{tool} の受信に失敗しました: {error}"))
        })?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| {
            DownloadError::Permanent(format!("{tool} を保存できません: {error}"))
        })?;
        hasher.update(&buffer[..read]);
        received += read as u64;

        if last_update.elapsed() >= Duration::from_millis(100) {
            last_update = Instant::now();
            let _ = events.send_blocking(BootstrapEvent::Downloading {
                tool: tool.to_string(),
                received,
                total,
                step,
                steps,
            });
        }
    }

    file.flush()
        .map_err(|error| DownloadError::Permanent(format!("{tool} を保存できません: {error}")))?;
    let _ = events.send_blocking(BootstrapEvent::Downloading {
        tool: tool.to_string(),
        received,
        total: total.or(Some(received)),
        step,
        steps,
    });
    Ok(hex(&hasher.finalize()))
}

fn classify_download_error(url: &str, error: ureq::Error) -> DownloadError {
    match error {
        ureq::Error::StatusCode(code) => DownloadError::Permanent(format!(
            "ダウンロード先が HTTP {code} を返しました（{url}）"
        )),
        ureq::Error::BadUri(message) => {
            DownloadError::Permanent(format!("ダウンロード先の URL が不正です: {message}"))
        }
        ureq::Error::Http(message) => {
            DownloadError::Permanent(format!("ダウンロードの HTTP 処理に失敗しました: {message}"))
        }
        ureq::Error::InvalidProxyUrl => {
            DownloadError::Permanent("プロキシの設定が不正です。".to_string())
        }
        other => DownloadError::Retryable(format!("{other}")),
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build(),
    )
}

enum DownloadError {
    Retryable(String),
    Permanent(String),
}

fn extract_expected_files(archive: &Path, destination: &Path, tool: &str) -> Result<(), String> {
    let expected = expected_files(tool);
    let expected_names: Vec<String> = expected.iter().map(|name| executable_name(name)).collect();
    let file =
        File::open(archive).map_err(|error| format!("{tool} の配布物を開けません: {error}"))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| format!("{tool} の配布物を読み取れません: {error}"))?;
    let mut found = HashSet::new();

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("{tool} の配布物を読み取れません: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let is_expected = expected_names.iter().any(|expected| expected == name);
        // ライセンス表記はアプリ内へ再配布するわけではないが、取得したツールの
        // 付属文書として一緒に残す。無い配布物もあるため必須にはしない。
        let is_license = name.eq_ignore_ascii_case("LICENSE");
        if !is_expected && !is_license {
            continue;
        }
        let target = destination.join(name);
        let mut output = File::create(&target)
            .map_err(|error| format!("{tool} の {name} を作成できません: {error}"))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| format!("{tool} の {name} を展開できません: {error}"))?;
        if is_expected {
            set_executable(&target)?;
            found.insert(name.to_string());
        }
    }

    for name in expected_names {
        if !found.contains(&name) {
            return Err(format!("{tool} の配布物に {name} が含まれていません。"));
        }
    }
    Ok(())
}

fn expected_files(tool: &str) -> &'static [&'static str] {
    match tool {
        YT_DLP => &["yt-dlp"],
        FFMPEG => &["ffmpeg", "ffprobe"],
        DENO => &["deno"],
        _ => &[],
    }
}

fn executable_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("{} に実行権限を付与できません: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn remove_path(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn cleanup_temporary_files(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !(name.starts_with(".download-")
            || name.starts_with(".staging-")
            || name.starts_with(".backup-")
            || name.starts_with(".installed-"))
        {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

/// 既定の配置先に記録されたツールのバージョン。About 画面の表示に使う。
pub fn installed_version(name: &str) -> Option<String> {
    ToolPaths::from_env().installed_version(name)
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, platform: &str, required: bool) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            platform: platform.to_string(),
            url: format!("https://example.com/{name}.zip"),
            sha256: "a".repeat(64),
            required,
        }
    }

    fn info(tools: Vec<ToolEntry>) -> ToolsInfo {
        ToolsInfo {
            schema_version: SCHEMA_VERSION,
            tools,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tmd-tools-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn selects_required_entries_for_the_current_platform() {
        let platform = Platform {
            os: "macos",
            arch: "aarch64",
        };
        let info = info(vec![
            entry(YT_DLP, "macos-aarch64", true),
            entry(FFMPEG, "macos-aarch64", true),
            entry(DENO, "macos-aarch64", true),
            entry(YT_DLP, "windows-x86_64", true),
            entry("optional", "macos-aarch64", false),
        ]);

        let selected = required_entries(&info, platform).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec![YT_DLP, FFMPEG, DENO]
        );
    }

    #[test]
    fn ignores_tools_the_app_does_not_know_yet() {
        let platform = Platform {
            os: "macos",
            arch: "aarch64",
        };
        let info = info(vec![
            entry(YT_DLP, "macos-aarch64", true),
            entry(FFMPEG, "macos-aarch64", true),
            entry(DENO, "macos-aarch64", true),
            entry("future-tool", "macos-aarch64", true),
        ]);

        let selected = required_entries(&info, platform).unwrap();
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn rejects_a_manifest_without_ffmpeg_for_the_platform() {
        let platform = Platform {
            os: "linux",
            arch: "x86_64",
        };
        let info = info(vec![entry(YT_DLP, "linux-x86_64", true)]);

        let error = required_entries(&info, platform).unwrap_err();
        assert!(error.contains("ffmpeg"));
    }

    #[test]
    fn rejects_a_manifest_without_deno_for_the_platform() {
        let platform = Platform {
            os: "macos",
            arch: "aarch64",
        };
        let info = info(vec![
            entry(YT_DLP, "macos-aarch64", true),
            entry(FFMPEG, "macos-aarch64", true),
        ]);

        let error = required_entries(&info, platform).unwrap_err();
        assert!(error.contains(DENO));
    }

    #[test]
    fn rejects_a_manifest_with_an_invalid_checksum() {
        let platform = Platform {
            os: "macos",
            arch: "aarch64",
        };
        let mut invalid = entry(YT_DLP, "macos-aarch64", true);
        invalid.sha256 = "not-a-checksum".to_string();
        let info = info(vec![
            invalid,
            entry(FFMPEG, "macos-aarch64", true),
            entry(DENO, "macos-aarch64", true),
        ]);

        let error = required_entries(&info, platform).unwrap_err();
        assert!(error.contains("チェックサム"));
    }

    #[test]
    fn extracts_only_the_expected_binaries_and_sets_them_executable() {
        let root = temp_dir("extract");
        let archive = root.join("tool.zip");
        write_zip(
            &archive,
            &[
                ("yt-dlp", b"binary"),
                ("README.md", b"readme"),
                ("nested/LICENSE", b"license"),
            ],
        );

        let destination = root.join("staging");
        fs::create_dir_all(&destination).unwrap();
        extract_expected_files(&archive, &destination, YT_DLP).unwrap();

        assert_eq!(fs::read(destination.join("yt-dlp")).unwrap(), b"binary");
        assert!(!destination.join("README.md").exists());
        assert_eq!(fs::read(destination.join("LICENSE")).unwrap(), b"license");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(destination.join("yt-dlp"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o755, 0o755);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_files_without_execute_permission() {
        let root = temp_dir("non-executable");
        let binary = root.join("deno");
        fs::write(&binary, b"binary").unwrap();

        assert!(!is_executable_file(&binary));
        set_executable(&binary).unwrap();
        assert!(is_executable_file(&binary));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_deno_and_sets_it_executable() {
        let root = temp_dir("extract-deno");
        let archive = root.join("deno.zip");
        write_zip(&archive, &[("deno", b"binary"), ("README.md", b"readme")]);

        let destination = root.join("staging");
        fs::create_dir_all(&destination).unwrap();
        extract_expected_files(&archive, &destination, DENO).unwrap();

        assert_eq!(fs::read(destination.join("deno")).unwrap(), b"binary");
        assert!(!destination.join("README.md").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(destination.join("deno"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o755, 0o755);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fails_when_the_archive_is_missing_a_binary() {
        let root = temp_dir("missing");
        let archive = root.join("tool.zip");
        write_zip(&archive, &[("ffmpeg", b"binary")]);

        let destination = root.join("staging");
        fs::create_dir_all(&destination).unwrap();
        let error = extract_expected_files(&archive, &destination, FFMPEG).unwrap_err();
        assert!(error.contains("ffprobe"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn records_and_reads_installed_versions() {
        let root = temp_dir("installed");
        let paths = ToolPaths::at(&root);
        let mut installed = InstalledTools::default();
        installed.set(
            YT_DLP,
            InstalledTool {
                version: "2026.10.26".to_string(),
                platform: "macos-aarch64".to_string(),
            },
        );
        paths.save_installed(&installed).unwrap();

        assert_eq!(
            paths.installed_version(YT_DLP).as_deref(),
            Some("2026.10.26")
        );
        assert!(paths.installed_version(FFMPEG).is_none());
        assert!(paths.installed_version(DENO).is_none());

        let _ = fs::remove_dir_all(root);
    }

    fn write_executable(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        set_executable(path).unwrap();
    }

    fn write_complete_tools(paths: &ToolPaths) {
        write_executable(&paths.yt_dlp(), b"old yt-dlp");
        write_executable(&paths.ffmpeg(), b"old ffmpeg");
        write_executable(&paths.ffprobe(), b"old ffprobe");
        write_executable(&paths.deno(), b"old deno");
    }

    fn record_versions(paths: &ToolPaths, yt_dlp: &str, ffmpeg: &str, deno: &str) {
        let mut installed = InstalledTools::default();
        let platform = Platform::current().unwrap().id();
        for (name, version) in [(YT_DLP, yt_dlp), (FFMPEG, ffmpeg), (DENO, deno)] {
            installed.set(
                name,
                InstalledTool {
                    version: version.to_string(),
                    platform: platform.clone(),
                },
            );
        }
        paths.save_installed(&installed).unwrap();
    }

    fn current_entry(name: &str) -> ToolEntry {
        entry(name, &Platform::current().unwrap().id(), true)
    }

    #[test]
    fn finds_no_updates_when_versions_and_binaries_match() {
        let root = temp_dir("up-to-date");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let entries = vec![
            current_entry(YT_DLP),
            current_entry(FFMPEG),
            current_entry(DENO),
        ];

        let pending = pending_entries(&paths, &paths.load_installed(), &entries);

        assert!(pending.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selects_only_the_tool_with_a_different_version() {
        let root = temp_dir("version-change");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let mut yt_dlp = current_entry(YT_DLP);
        yt_dlp.version = "2.0.0".to_string();
        let entries = vec![yt_dlp, current_entry(FFMPEG), current_entry(DENO)];

        let pending = pending_entries(&paths, &paths.load_installed(), &entries);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, YT_DLP);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selects_a_tool_when_its_binary_is_missing() {
        let root = temp_dir("missing-binary");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        fs::remove_file(paths.ffprobe()).unwrap();
        let entries = vec![
            current_entry(YT_DLP),
            current_entry(FFMPEG),
            current_entry(DENO),
        ];

        let pending = pending_entries(&paths, &paths.load_installed(), &entries);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, FFMPEG);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selects_deno_when_its_binary_is_missing() {
        let root = temp_dir("missing-deno");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        fs::remove_file(paths.deno()).unwrap();
        let entries = vec![
            current_entry(YT_DLP),
            current_entry(FFMPEG),
            current_entry(DENO),
        ];

        let pending = pending_entries(&paths, &paths.load_installed(), &entries);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, DENO);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selects_a_tool_when_its_recorded_platform_differs() {
        let root = temp_dir("platform-change");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let mut yt_dlp = current_entry(YT_DLP);
        yt_dlp.platform = "another-platform".to_string();
        let entries = vec![yt_dlp, current_entry(FFMPEG), current_entry(DENO)];

        let pending = pending_entries(&paths, &paths.load_installed(), &entries);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, YT_DLP);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checks_for_updates_even_when_all_tools_are_installed() {
        let root = temp_dir("check-updates");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let platform = Platform::current().unwrap().id();
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "tools": [
                {
                    "name": YT_DLP,
                    "version": "2.0.0",
                    "platform": platform,
                    "url": "https://example.com/yt-dlp.zip",
                    "sha256": "a".repeat(64),
                    "required": true
                },
                {
                    "name": FFMPEG,
                    "version": "1.0.0",
                    "platform": platform,
                    "url": "https://example.com/ffmpeg.zip",
                    "sha256": "b".repeat(64),
                    "required": true
                },
                {
                    "name": DENO,
                    "version": "1.0.0",
                    "platform": platform,
                    "url": "https://example.com/deno.zip",
                    "sha256": "c".repeat(64),
                    "required": true
                }
            ]
        });
        let (url, server) = serve_once(serde_json::to_vec(&manifest).unwrap());

        let updates = check_for_tool_updates_with_url(&paths, &url).unwrap();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].name, YT_DLP);
        assert_eq!(updates[0].version, "2.0.0");
        server.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_update_selection_finishes_without_installing() {
        let root = temp_dir("empty-selection");
        let paths = ToolPaths::at(&root);
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let (sender, receiver) = smol::channel::unbounded();

        install_tool_updates(paths, Vec::new(), sender);

        assert_eq!(receiver.try_recv().unwrap(), BootstrapEvent::Ready);
        let _ = fs::remove_dir_all(root);
    }

    fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        });
        (format!("http://{address}/tool.zip"), handle)
    }

    fn sha256_bytes(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex(&hasher.finalize())
    }

    #[test]
    fn downloads_verifies_and_installs_a_zip() {
        let root = temp_dir("download");
        let archive = root.join("fixture.zip");
        write_zip(&archive, &[("yt-dlp", b"#!/bin/sh\necho test\n")]);
        let bytes = fs::read(&archive).unwrap();
        let (url, server) = serve_once(bytes.clone());
        let paths = ToolPaths::at(root.join("tools"));
        fs::create_dir_all(paths.root()).unwrap();
        let entry = ToolEntry {
            name: YT_DLP.to_string(),
            version: "1.0.0".to_string(),
            platform: "macos-aarch64".to_string(),
            url,
            sha256: sha256_bytes(&bytes),
            required: true,
        };
        let (sender, _receiver) = smol::channel::unbounded();

        install_entry(&paths, &entry, &sender, 1, 1).unwrap();

        assert!(is_executable_file(&paths.yt_dlp()));
        server.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn installs_only_the_selected_tool_and_preserves_the_other() {
        let root = temp_dir("selective-install");
        let archive = root.join("fixture.zip");
        write_zip(&archive, &[("yt-dlp", b"new yt-dlp")]);
        let bytes = fs::read(&archive).unwrap();
        let (url, server) = serve_once(bytes.clone());
        let paths = ToolPaths::at(root.join("tools"));
        write_complete_tools(&paths);
        record_versions(&paths, "1.0.0", "1.0.0", "1.0.0");
        let entry = ToolEntry {
            name: YT_DLP.to_string(),
            version: "2.0.0".to_string(),
            platform: Platform::current().unwrap().id(),
            url,
            sha256: sha256_bytes(&bytes),
            required: true,
        };
        let (sender, _receiver) = smol::channel::unbounded();

        install_entries(&paths, &[entry], &sender).unwrap();

        assert_eq!(fs::read(paths.yt_dlp()).unwrap(), b"new yt-dlp");
        assert_eq!(fs::read(paths.ffmpeg()).unwrap(), b"old ffmpeg");
        assert_eq!(fs::read(paths.ffprobe()).unwrap(), b"old ffprobe");
        assert_eq!(fs::read(paths.deno()).unwrap(), b"old deno");
        let installed = paths.load_installed();
        assert_eq!(installed.get(YT_DLP).unwrap().version, "2.0.0");
        assert_eq!(installed.get(FFMPEG).unwrap().version, "1.0.0");
        assert_eq!(installed.get(DENO).unwrap().version, "1.0.0");
        server.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_download_with_a_mismatched_checksum() {
        let root = temp_dir("checksum");
        let archive = root.join("fixture.zip");
        write_zip(&archive, &[("yt-dlp", b"#!/bin/sh\necho test\n")]);
        let (url, server) = serve_once(fs::read(&archive).unwrap());
        let paths = ToolPaths::at(root.join("tools"));
        fs::create_dir_all(paths.root()).unwrap();
        let entry = ToolEntry {
            name: YT_DLP.to_string(),
            version: "1.0.0".to_string(),
            platform: "macos-aarch64".to_string(),
            url,
            sha256: "0".repeat(64),
            required: true,
        };
        let (sender, _receiver) = smol::channel::unbounded();

        let error = install_entry(&paths, &entry, &sender, 1, 1).unwrap_err();

        assert!(error.contains("チェックサム"));
        assert!(!paths.yt_dlp().exists());
        server.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_the_release_manifest() {
        let json = r#"{
            "schemaVersion": 1,
            "tools": [
                {
                    "name": "yt-dlp",
                    "version": "2026.10.26",
                    "platform": "macos-aarch64",
                    "url": "https://example.com/yt-dlp.zip",
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "required": true
                },
                {
                    "name": "ffmpeg",
                    "version": "b6.1.1",
                    "platform": "macos-aarch64",
                    "url": "https://example.com/ffmpeg.zip",
                    "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "required": true
                },
                {
                    "name": "deno",
                    "version": "v2.9.8",
                    "platform": "macos-aarch64",
                    "url": "https://example.com/deno.zip",
                    "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "required": true
                }
            ]
        }"#;

        let info: ToolsInfo = serde_json::from_str(json).unwrap();
        let entries = required_entries(
            &info,
            Platform {
                os: "macos",
                arch: "aarch64",
            },
        )
        .unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, YT_DLP);
        assert_eq!(entries[0].version, "2026.10.26");
        assert_eq!(entries[1].name, FFMPEG);
        assert_eq!(entries[1].version, "b6.1.1");
        assert_eq!(entries[2].name, DENO);
        assert_eq!(entries[2].version, "v2.9.8");
    }

    #[test]
    fn computes_the_sha256_hex_of_downloaded_bytes() {
        let mut hasher = Sha256::new();
        hasher.update(b"hello");
        assert_eq!(
            hex(&hasher.finalize()),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
