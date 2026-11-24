//! アプリ本体の更新確認・ダウンロード・配置。
//!
//! GitHub Releases の `update-info.json` を取得し、実行中のバージョンより新しければ
//! 配布ZIPをダウンロードして `.app` を置き換える。ネットワークとファイル操作は
//! UI を止めないよう呼び出し側のスレッドで実行し、進捗はイベントで通知する。

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::app_settings::APP_VERSION;

/// 最新リリースの更新情報。
pub const UPDATE_INFO_URL: &str =
    "https://github.com/treetips/tree-movie-downloader/releases/latest/download/update-info.json";

const UPDATE_INFO_URL_ENV: &str = "TMD_UPDATE_INFO_URL";
const MAX_DOWNLOAD_RETRIES: u32 = 3;

/// `update-info.json` の内容。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// セマンティックバージョン（Major.Minor.Patch）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let value = value.strip_prefix('v').unwrap_or(value);
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// インストール処理の進捗。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallEvent {
    Downloading { received: u64, total: Option<u64> },
    Verifying,
    Extracting,
    Placing,
    Installed { app: PathBuf },
    Revealed { app: PathBuf },
    Failed(String),
}

/// 最新バージョンを取得し、更新が必要ならその情報を返す。
pub fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let info = fetch_update_info()?;
    let current = current_version()?;
    let latest = Version::parse(&info.version).ok_or_else(|| {
        format!(
            "更新情報のバージョン（{}）を読み取れませんでした。",
            info.version
        )
    })?;
    Ok((latest > current).then_some(info))
}

/// 実行中のアプリのバージョン。
pub fn current_version() -> Result<Version, String> {
    Version::parse(APP_VERSION)
        .ok_or_else(|| format!("現在のアプリバージョン（{APP_VERSION}）を読み取れませんでした。"))
}

fn update_info_url() -> String {
    std::env::var(UPDATE_INFO_URL_ENV).unwrap_or_else(|_| UPDATE_INFO_URL.to_string())
}

fn fetch_update_info() -> Result<UpdateInfo, String> {
    let url = update_info_url();
    let agent = agent(Duration::from_secs(10));
    let mut response = match agent.get(&url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!("更新情報が HTTP {code} を返しました（{url}）。"));
        }
        Err(error) => {
            return Err(format!("更新情報を取得できませんでした（{url}）: {error}"));
        }
    };
    let text = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(|error| format!("更新情報を読み取れませんでした: {error}"))?;
    let info: UpdateInfo = serde_json::from_str(&text)
        .map_err(|error| format!("更新情報の形式が正しくありません: {error}"))?;
    validate_info(&info)?;
    Ok(info)
}

fn validate_info(info: &UpdateInfo) -> Result<(), String> {
    if Version::parse(&info.version).is_none() {
        return Err(format!(
            "更新情報のバージョン（{}）が不正です。",
            info.version
        ));
    }
    if !info.url.starts_with("https://") {
        return Err("更新情報の配布元が https ではありません。".to_string());
    }
    if info.sha256.len() != 64 || !info.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("更新情報のチェックサムが不正です。".to_string());
    }
    Ok(())
}

/// 更新ZIPをダウンロードし、検証・展開・配置まで行う。呼び出し側は専用スレッドで実行する。
pub fn install_update(info: UpdateInfo, events: smol::channel::Sender<InstallEvent>) {
    match install_inner(&info, &events) {
        Ok(InstallOutcome::Installed(app)) => {
            let _ = events.send_blocking(InstallEvent::Installed { app });
        }
        Ok(InstallOutcome::Revealed(app)) => {
            let _ = events.send_blocking(InstallEvent::Revealed { app });
        }
        Err(message) => {
            let _ = events.send_blocking(InstallEvent::Failed(message));
        }
    }
}

enum InstallOutcome {
    Installed(PathBuf),
    Revealed(PathBuf),
}

fn install_inner(
    info: &UpdateInfo,
    events: &smol::channel::Sender<InstallEvent>,
) -> Result<InstallOutcome, String> {
    let running_app = running_app_bundle();
    let work_dir = match &running_app {
        Some(app) => app
            .parent()
            .ok_or_else(|| "アプリの親ディレクトリを特定できません。".to_string())?
            .join(format!(
                ".tree-movie-downloader-update-{}",
                std::process::id()
            )),
        None => std::env::temp_dir().join(format!(
            "tree-movie-downloader-update-{}-{}",
            std::process::id(),
            unix_nanos()
        )),
    };
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir)
        .map_err(|error| format!("更新用の一時ディレクトリを作成できません: {error}"))?;

    let result = install_in_work_dir(info, &running_app, &work_dir, events);

    match &result {
        Ok(InstallOutcome::Installed(_)) | Err(_) => {
            let _ = fs::remove_dir_all(&work_dir);
        }
        Ok(InstallOutcome::Revealed(_)) => {
            // Finderで確認してもらうため、展開したアプリを残す。
        }
    }
    result
}

fn install_in_work_dir(
    info: &UpdateInfo,
    running_app: &Option<PathBuf>,
    work_dir: &Path,
    events: &smol::channel::Sender<InstallEvent>,
) -> Result<InstallOutcome, String> {
    let zip_path = work_dir.join("update.zip");
    let actual = download_update(info, &zip_path, events)?;
    let _ = events.send_blocking(InstallEvent::Verifying);
    if !actual.eq_ignore_ascii_case(&info.sha256) {
        return Err("更新ZIPのチェックサムが一致しません。".to_string());
    }

    let _ = events.send_blocking(InstallEvent::Extracting);
    let extract_dir = work_dir.join("extracted");
    fs::create_dir_all(&extract_dir)
        .map_err(|error| format!("更新ZIPの展開先を作成できません: {error}"))?;
    run_command(
        "/usr/bin/ditto",
        &[
            "-x".into(),
            "-k".into(),
            zip_path.as_os_str().to_os_string(),
            extract_dir.as_os_str().to_os_string(),
        ],
    )?;

    let new_app = find_app_bundle(&extract_dir)
        .ok_or_else(|| "更新ZIPにアプリケーションバンドルが含まれていません。".to_string())?;
    verify_app(&new_app)?;

    match running_app {
        Some(target) => {
            let _ = events.send_blocking(InstallEvent::Placing);
            replace_app(target, &new_app)?;
            launch_app(target)?;
            Ok(InstallOutcome::Installed(target.clone()))
        }
        None => Ok(InstallOutcome::Revealed(new_app)),
    }
}

fn download_update(
    info: &UpdateInfo,
    destination: &Path,
    events: &smol::channel::Sender<InstallEvent>,
) -> Result<String, String> {
    let agent = agent(Duration::from_secs(30 * 60));
    let mut last_error = String::new();

    for attempt in 0..=MAX_DOWNLOAD_RETRIES {
        let _ = fs::remove_file(destination);
        let _ = events.send_blocking(InstallEvent::Downloading {
            received: 0,
            total: None,
        });
        match download_once(&agent, info, destination, events) {
            Ok(actual) => return Ok(actual),
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
        "更新ZIPをダウンロードできませんでした（{MAX_DOWNLOAD_RETRIES}回再試行）: {last_error}"
    ))
}

fn download_once(
    agent: &ureq::Agent,
    info: &UpdateInfo,
    destination: &Path,
    events: &smol::channel::Sender<InstallEvent>,
) -> Result<String, DownloadError> {
    let response = agent
        .get(&info.url)
        .call()
        .map_err(|error| classify_download_error(&info.url, error))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(destination)
        .map_err(|error| DownloadError::Permanent(format!("更新ZIPを保存できません: {error}")))?;

    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    let mut last_update = Instant::now();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            DownloadError::Retryable(format!("更新ZIPの受信に失敗しました: {error}"))
        })?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| {
            DownloadError::Permanent(format!("更新ZIPを保存できません: {error}"))
        })?;
        hasher.update(&buffer[..read]);
        received += read as u64;

        if last_update.elapsed() >= Duration::from_millis(100) {
            last_update = Instant::now();
            let _ = events.send_blocking(InstallEvent::Downloading { received, total });
        }
    }

    file.flush()
        .map_err(|error| DownloadError::Permanent(format!("更新ZIPを保存できません: {error}")))?;
    let _ = events.send_blocking(InstallEvent::Downloading {
        received,
        total: total.or(Some(received)),
    });
    Ok(hex(&hasher.finalize()))
}

fn classify_download_error(url: &str, error: ureq::Error) -> DownloadError {
    match error {
        ureq::Error::StatusCode(code) => DownloadError::Permanent(format!(
            "更新ZIPの配布元が HTTP {code} を返しました（{url}）"
        )),
        ureq::Error::BadUri(message) => {
            DownloadError::Permanent(format!("更新ZIPのURLが不正です: {message}"))
        }
        ureq::Error::Http(message) => {
            DownloadError::Permanent(format!("更新ZIPのHTTP処理に失敗しました: {message}"))
        }
        ureq::Error::InvalidProxyUrl => {
            DownloadError::Permanent("プロキシの設定が不正です。".to_string())
        }
        other => DownloadError::Retryable(format!("{other}")),
    }
}

enum DownloadError {
    Retryable(String),
    Permanent(String),
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build(),
    )
}

fn run_command(program: &str, args: &[OsString]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("{program} を実行できません: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(format!(
        "{program} が終了コード {} で終了しました{}{}",
        output.status.code().unwrap_or(-1),
        if stderr.is_empty() { "" } else { ": " },
        stderr
    ))
}

fn find_app_bundle(directory: &Path) -> Option<PathBuf> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
}

fn verify_app(app: &Path) -> Result<(), String> {
    run_command(
        "/usr/bin/codesign",
        &[
            "--verify".into(),
            "--deep".into(),
            "--strict".into(),
            app.as_os_str().to_os_string(),
        ],
    )
    .map_err(|error| format!("新しいアプリの署名を検証できません: {error}"))
}

fn replace_app(target: &Path, new_app: &Path) -> Result<(), String> {
    let backup = target.with_file_name(format!(
        "{}.backup-{}",
        target
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "app".to_string()),
        unix_nanos()
    ));

    fs::rename(target, &backup).map_err(|error| {
        format!("現在のアプリを退避できません（管理者権限が必要な場合があります）: {error}")
    })?;

    if let Err(error) = fs::rename(new_app, target) {
        let _ = fs::rename(&backup, target);
        return Err(format!("新しいアプリを配置できません: {error}"));
    }

    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn launch_app(app: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg("-n")
        .arg(app)
        .status()
        .map_err(|error| format!("新しいアプリを起動できません: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "新しいアプリを起動できませんでした（終了コード {}）。",
            status.code().unwrap_or(-1)
        ))
    }
}

/// 実行中のバイナリが `.app` の中にある場合、そのバンドルのパスを返す。
pub fn running_app_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    app_bundle_for(&executable)
}

fn app_bundle_for(executable: &Path) -> Option<PathBuf> {
    let macos_dir = executable.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }
    let app = contents_dir.parent()?;
    if app.to_string_lossy().contains("/AppTranslocation/") {
        // GatekeeperのApp Translocation中は読み取り専用のため、自動置き換えしない。
        return None;
    }
    app.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        .then(|| app.to_path_buf())
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
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

    #[test]
    fn parses_and_compares_versions() {
        let current = Version::parse("0.1.0").unwrap();
        assert!(Version::parse("0.1.1").unwrap() > current);
        assert!(Version::parse("0.2.0").unwrap() > current);
        assert!(Version::parse("v1.0.0").unwrap() > current);
        assert!(Version::parse("0.1.0").unwrap() == current);
        assert!(Version::parse("0.1").is_none());
        assert!(Version::parse("0.1.0.1").is_none());
        assert!(Version::parse("dev").is_none());
    }

    #[test]
    fn validates_update_info() {
        let info = UpdateInfo {
            version: "0.2.0".to_string(),
            url: "https://example.com/app.zip".to_string(),
            sha256: "a".repeat(64),
        };
        assert!(validate_info(&info).is_ok());

        let mut invalid = info.clone();
        invalid.version = "latest".to_string();
        assert!(validate_info(&invalid).is_err());

        let mut invalid = info.clone();
        invalid.url = "http://example.com/app.zip".to_string();
        assert!(validate_info(&invalid).is_err());

        let mut invalid = info;
        invalid.sha256 = "abc".to_string();
        assert!(validate_info(&invalid).is_err());
    }

    #[test]
    fn parses_update_info_json() {
        let info: UpdateInfo = serde_json::from_str(
            r#"{"version":"0.2.0","url":"https://example.com/app.zip","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        )
        .unwrap();
        assert_eq!(info.version, "0.2.0");
    }

    #[test]
    fn identifies_a_macos_app_bundle() {
        let root = std::env::temp_dir().join(format!(
            "tmd-update-test-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let app = root.join("Tree Movie Downloader.app");
        let macos = app.join("Contents").join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        assert_eq!(
            app_bundle_for(&macos.join("tree-movie-downloader")),
            Some(app)
        );
        assert!(app_bundle_for(Path::new("/tmp/tree-movie-downloader")).is_none());
        let _ = fs::remove_dir_all(root);
    }

    fn create_fake_app(root: &Path) -> PathBuf {
        let app = root.join("Fake.app");
        let contents = app.join("Contents");
        let macos = contents.join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        let executable = macos.join("fake");
        fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.example.fake</string>
  <key>CFBundleExecutable</key><string>fake</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict>
</plist>
"#,
        )
        .unwrap();
        let status = Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(&app)
            .status()
            .unwrap();
        assert!(status.success(), "codesign failed");
        app
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
        (format!("http://{address}/update.zip"), handle)
    }

    #[test]
    fn replaces_an_existing_app_bundle() {
        let root = std::env::temp_dir().join(format!(
            "tmd-update-replace-test-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let target = root.join("Current.app");
        let new_app = root.join("New.app");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&new_app).unwrap();
        fs::write(target.join("old.txt"), b"old").unwrap();
        fs::write(new_app.join("new.txt"), b"new").unwrap();

        replace_app(&target, &new_app).unwrap();

        assert!(target.join("new.txt").is_file());
        assert!(!target.join("old.txt").exists());
        assert!(!new_app.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn downloads_verifies_and_reveals_an_update_for_a_dev_build() {
        let root = std::env::temp_dir().join(format!(
            "tmd-update-download-test-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let fake_app = create_fake_app(&root);
        let zip = root.join("update.zip");
        run_command(
            "/usr/bin/ditto",
            &[
                "-c".into(),
                "-k".into(),
                "--keepParent".into(),
                fake_app.as_os_str().to_os_string(),
                zip.as_os_str().to_os_string(),
            ],
        )
        .unwrap();
        let bytes = fs::read(&zip).unwrap();
        let (url, server) = serve_once(bytes.clone());
        let info = UpdateInfo {
            version: "0.2.0".to_string(),
            url,
            sha256: hex(&{
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                hasher.finalize()
            }),
        };

        let (sender, receiver) = smol::channel::unbounded();
        let install = std::thread::spawn(move || install_update(info, sender));
        let app = loop {
            let event = smol::block_on(receiver.recv()).unwrap();
            match event {
                InstallEvent::Revealed { app } => break app,
                InstallEvent::Failed(message) => panic!("install failed: {message}"),
                _ => {}
            }
        };
        install.join().unwrap();
        server.join().unwrap();

        assert!(app.is_dir());
        if let Some(work_dir) = app.parent().and_then(|path| path.parent()) {
            let _ = fs::remove_dir_all(work_dir);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn finds_the_app_bundle_in_an_extracted_directory() {
        let root = std::env::temp_dir().join(format!(
            "tmd-update-find-test-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let app = root.join("Tree Movie Downloader.app");
        fs::create_dir_all(&app).unwrap();
        assert_eq!(find_app_bundle(&root), Some(app));
        let _ = fs::remove_dir_all(root);
    }
}
