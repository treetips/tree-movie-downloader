//! yt-dlp の実行と、サイトごとのダウンロード制御。

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::{AppContext as _, AsyncApp, Context, Entity, SharedString};
use serde::Deserialize;

use crate::app_settings::{AppSettings, DEFAULT_FILENAME_TEMPLATE, play_sound, scan_sounds};
use crate::file_actions::expand_path;
use crate::model::{
    AudioQuality, Container, ItemStatus, Quality, QueueEvent, QueueItem, QueueModel, Site,
    compact_url,
};
use crate::thumbnail::load_thumbnail;
use crate::tools::ToolPaths;

/// 出力がこの時間途切れたら、yt-dlpが固まったとみなして停止する。
///
/// 進捗はダウンロード中に頻繁に出るが、ffmpegによる変換中は完了まで
/// 出力が止まることがあるため、変換時間を見込んで長めにしている。
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// サイトごとのダウンロード制御。
pub struct DownloadManager {
    queue: Entity<QueueModel>,
    active: HashMap<Site, ActiveDownload>,
    expanded_logs: HashSet<Site>,
}

struct ActiveDownload {
    id: u64,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
}

#[derive(Clone)]
struct DownloadTask {
    id: u64,
    url: SharedString,
    quality: Quality,
    audio: AudioQuality,
    container: Container,
    destination: String,
    filename_template: String,
    cookies_from_browser: Option<String>,
    retries: usize,
    single_item: bool,
    force: bool,
}

enum DownloadEvent {
    Progress {
        id: u64,
        percent: f32,
        downloaded: String,
        total: String,
        eta: Option<String>,
    },
    Completed {
        id: u64,
        url: String,
        path: Option<String>,
    },
    Failed {
        id: u64,
        url: String,
        error: String,
    },
    Skipped {
        id: u64,
        path: String,
        title: Option<String>,
        thumbnail_url: Option<String>,
    },
    Metadata {
        id: u64,
        title: Option<String>,
        thumbnail_url: Option<String>,
    },
    Warning {
        id: u64,
        message: String,
    },
    Cancelled {
        id: u64,
    },
}

enum RunOutcome {
    Success(Option<String>),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Deserialize)]
struct PrintedMetadata {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    filepath: Option<String>,
}

struct ExistingDownload {
    path: String,
    title: Option<String>,
    thumbnail_url: Option<String>,
}

impl DownloadManager {
    pub fn new(queue: Entity<QueueModel>) -> Self {
        Self {
            queue,
            active: HashMap::new(),
            expanded_logs: HashSet::new(),
        }
    }

    /// サイトの開始・一時停止を切り替える。
    pub fn toggle_site(&mut self, site: Site, cx: &mut Context<Self>) {
        if self.active.contains_key(&site) {
            self.cancel_site(site);
            return;
        }
        if self.queue.read(cx).site_is_running(site) {
            self.queue.update(cx, |model, cx| {
                model.pause_site(site);
                cx.emit(QueueEvent::Changed);
                cx.notify();
            });
            return;
        }
        self.start_site(site, cx);
    }

    /// 待機中・一時停止の項目があるサイトを開始する。ツールバーの `開始する` から使う。
    pub fn start_pending_sites(&mut self, cx: &mut Context<Self>) {
        for site in Site::ALL {
            let has_pending = self.queue.read(cx).items().iter().any(|item| {
                item.site() == site
                    && matches!(item.status(), ItemStatus::Paused | ItemStatus::Queued)
            });
            if has_pending {
                self.start_site(site, cx);
            }
        }
    }

    pub fn log_expanded(&self, site: Site) -> bool {
        self.expanded_logs.contains(&site)
    }

    pub fn toggle_log(&mut self, site: Site, cx: &mut Context<Self>) {
        if !self.expanded_logs.insert(site) {
            self.expanded_logs.remove(&site);
        }
        cx.notify();
    }

    /// 全サイトを開始・一時停止する。キーボードショートカット用。
    pub fn toggle_all(&mut self, cx: &mut Context<Self>) {
        if self
            .active
            .values()
            .any(|active| !active.cancel.load(Ordering::SeqCst))
            || self.queue.read(cx).active_item().is_some()
        {
            for site in self.active.keys().copied().collect::<Vec<_>>() {
                self.cancel_site(site);
            }
            self.queue.update(cx, |model, cx| {
                model.set_running(false);
                cx.emit(QueueEvent::Changed);
                cx.notify();
            });
        } else {
            for site in Site::ALL {
                self.start_site(site, cx);
            }
        }
    }

    /// 実行中のプロセスを停止する。キュー項目自体は削除しない。
    pub fn cancel_site(&mut self, site: Site) {
        if let Some(active) = self.active.get(&site) {
            cancel_active(active);
        }
    }

    /// 指定した項目を直接開始する。完了後に同サイトの後続項目は自動開始しない。
    pub fn start_item(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        self.start_item_with_force(id, false, cx)
    }

    /// 待機中・一時停止中の項目を、既存ファイルの上書きを許可して開始する。
    ///
    /// 完了・スキップ済み項目は、呼び出し側で先に再ダウンロード用の項目へ
    /// 置き換えてから、その新しい識別子を渡す必要がある。
    pub fn start_forced_item(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        self.start_item_with_force(id, true, cx)
    }

    fn start_item_with_force(&mut self, id: u64, force: bool, cx: &mut Context<Self>) -> bool {
        let Some(item) = self.queue.read(cx).item(id) else {
            return false;
        };
        if !matches!(item.status(), ItemStatus::Paused | ItemStatus::Queued) {
            return false;
        }
        self.start_task(item.site(), Some(id), false, force, cx)
    }

    /// 現在実行中の指定項目だけをキャンセルする。
    pub fn cancel_item(&mut self, id: u64, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(item) = self.queue.read(cx).item(id) else {
            return Err("キャンセル対象の項目が見つかりません。".into());
        };
        if item.status() != ItemStatus::Downloading {
            return Err("実行中の項目だけをキャンセルできます。".into());
        }
        let site = item.site();
        let Some(active) = self.active.get(&site) else {
            return Err("実行中のプロセスが見つかりません。".into());
        };
        if active.id != id {
            return Err("指定した項目は現在実行中ではありません。".into());
        }
        cancel_active(active);
        Ok(())
    }

    fn start_site(&mut self, site: Site, cx: &mut Context<Self>) {
        let _ = self.start_task(site, None, true, false, cx);
    }

    fn start_task(
        &mut self,
        site: Site,
        id: Option<u64>,
        continue_site: bool,
        force: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active.contains_key(&site) || self.queue.read(cx).site_is_running(site) {
            return false;
        }

        let settings = AppSettings::load();
        let filename_template = if settings.filename_template.trim().is_empty() {
            DEFAULT_FILENAME_TEMPLATE.to_string()
        } else {
            settings.filename_template.clone()
        };
        let cookies_from_browser = env_cookies_from_browser().or_else(|| {
            settings
                .cookies_from_browser
                .yt_dlp_value()
                .map(str::to_string)
        });
        let retries = self.queue.read(cx).settings().retries().max(1);

        let task = self.queue.update(cx, |model, cx| {
            let item = match id {
                Some(id) => model.take_item_for_site(site, id),
                None => model.take_next_for_site(site),
            }?;
            cx.emit(QueueEvent::Changed);
            cx.notify();
            Some(task_from_item(
                item,
                filename_template,
                cookies_from_browser,
                retries,
                force,
            ))
        });
        let Some(task) = task else {
            return false;
        };

        let (tx, rx) = smol::channel::unbounded::<DownloadEvent>();
        let cancel = Arc::new(AtomicBool::new(false));
        let pid = Arc::new(AtomicU32::new(0));
        self.active.insert(
            site,
            ActiveDownload {
                id: task.id,
                cancel: cancel.clone(),
                pid: pid.clone(),
            },
        );
        cx.notify();

        let worker_task = task.clone();
        let worker_tx = tx.clone();
        thread::spawn(move || {
            run_worker(worker_task, cancel, pid, worker_tx);
        });

        let queue = self.queue.clone();
        let manager = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let mut should_start_next = false;
            while let Ok(event) = rx.recv().await {
                let feedback = terminal_feedback(&event);
                match event {
                    DownloadEvent::Progress {
                        id,
                        percent,
                        downloaded,
                        total,
                        eta,
                    } => {
                        queue.update(cx, |model, cx| {
                            model.set_item_progress(
                                id,
                                percent,
                                downloaded.into(),
                                total.into(),
                                eta.map(Into::into),
                            );
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                    }
                    DownloadEvent::Completed { id, path, .. } => {
                        queue.update(cx, |model, cx| {
                            model.complete_item(id, path.as_deref());
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                        report_terminal_feedback(feedback);
                        should_start_next = true;
                        break;
                    }
                    DownloadEvent::Failed { id, error, .. } => {
                        queue.update(cx, |model, cx| {
                            model.fail_item(id, error);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                        report_terminal_feedback(feedback);
                        should_start_next = true;
                        break;
                    }
                    DownloadEvent::Skipped {
                        id,
                        path,
                        title,
                        thumbnail_url,
                    } => {
                        queue.update(cx, |model, cx| {
                            model.set_item_metadata(id, title.as_deref(), thumbnail_url.as_deref());
                            model.skip_already_downloaded(id, &path);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                        if let Some(url) = thumbnail_url.filter(|url| !url.trim().is_empty()) {
                            spawn_thumbnail_load(queue.clone(), id, url, cx);
                        }
                        report_terminal_feedback(feedback);
                        should_start_next = true;
                        break;
                    }
                    DownloadEvent::Metadata {
                        id,
                        title,
                        thumbnail_url,
                    } => {
                        queue.update(cx, |model, cx| {
                            model.set_item_metadata(id, title.as_deref(), thumbnail_url.as_deref());
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                        if let Some(url) = thumbnail_url.filter(|url| !url.trim().is_empty()) {
                            spawn_thumbnail_load(queue.clone(), id, url, cx);
                        }
                    }
                    DownloadEvent::Warning { id, message } => {
                        queue.update(cx, |model, cx| {
                            model.warn_item(id, message);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                    }
                    DownloadEvent::Cancelled { id } => {
                        queue.update(cx, |model, cx| {
                            model.pause_item(id);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                        break;
                    }
                }
            }

            let _ = manager.update(cx, |manager, cx| {
                manager.active.remove(&site);
                if should_start_next && continue_site {
                    manager.start_site(site, cx);
                }
                cx.notify();
            });
        })
        .detach();
        true
    }
}

fn spawn_thumbnail_load(queue: Entity<QueueModel>, id: u64, url: String, cx: &mut AsyncApp) {
    let thumbnail_queue = queue.clone();
    cx.spawn(async move |cx| {
        let thumbnail_url = url.clone();
        let image = cx
            .background_spawn(async move { load_thumbnail(&thumbnail_url) })
            .await;
        match image {
            Ok(image) => {
                thumbnail_queue.update(cx, |model, cx| {
                    if model.set_item_thumbnail(id, &url, image) {
                        cx.emit(QueueEvent::Changed);
                        cx.notify();
                    }
                });
            }
            Err(error) => {
                thumbnail_queue.update(cx, |model, cx| {
                    model.info_item(id, format!("サムネイルを表示できません: {error}"));
                    cx.emit(QueueEvent::Changed);
                    cx.notify();
                });
            }
        }
    })
    .detach();
}

fn task_from_item(
    item: QueueItem,
    filename_template: String,
    cookies_from_browser: Option<String>,
    retries: usize,
    force: bool,
) -> DownloadTask {
    DownloadTask {
        id: item.id(),
        url: item.url().clone(),
        quality: item.quality(),
        audio: item.audio(),
        container: item.container(),
        destination: item.destination().to_string(),
        filename_template,
        cookies_from_browser,
        retries,
        single_item: item.is_single_item(),
        force,
    }
}

/// 動画URLに `list=` が付いていても、意図しないミックス全体を取得しない。
///
/// プレイリストURL（`/playlist?list=...`）やチャンネルURLは対象外とし、
/// 従来どおり複数件の取得を許可する。
fn is_video_with_playlist(url: &str) -> bool {
    if !url.contains("list=") {
        return false;
    }
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let (host, path_and_query) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim_start_matches("www.").to_lowercase();
    let path = path_and_query.split(['?', '#']).next().unwrap_or_default();

    if host == "youtu.be" {
        return !path.is_empty();
    }
    if host == "youtube.com" || host.ends_with(".youtube.com") {
        return path == "watch"
            || path.starts_with("shorts/")
            || path.starts_with("live/")
            || path.starts_with("embed/");
    }
    false
}

fn cancel_active(active: &ActiveDownload) {
    active.cancel.store(true, Ordering::SeqCst);
    let pid = active.pid.load(Ordering::SeqCst);
    if pid > 0 {
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}

fn single_video_only(task: &DownloadTask) -> bool {
    task.single_item || is_video_with_playlist(&task.url)
}

fn playlist_items_arg(task: &DownloadTask) -> &'static str {
    if task.single_item { "1" } else { "1-2" }
}

fn apply_overwrite_args(command: &mut Command, task: &DownloadTask) {
    if task.force {
        command.arg("--force-overwrites");
    } else {
        command.arg("--no-overwrites");
    }
}

fn apply_single_item_args(command: &mut Command, task: &DownloadTask) {
    if single_video_only(task) {
        command.arg("--no-playlist");
    }
    if task.single_item {
        command
            .arg("--playlist-items")
            .arg(playlist_items_arg(task));
    }
}

/// 環境変数が設定されていれば、設定画面の値より優先する。
fn env_cookies_from_browser() -> Option<String> {
    std::env::var("YT_DLP_COOKIES_FROM_BROWSER")
        .ok()
        .map(|browser| browser.trim().to_string())
        .filter(|browser| !browser.is_empty())
}

fn run_worker(
    task: DownloadTask,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    tx: smol::channel::Sender<DownloadEvent>,
) {
    if cancel.load(Ordering::SeqCst) {
        let _ = tx.send_blocking(DownloadEvent::Cancelled { id: task.id });
        return;
    }

    if !task.force
        && let Some(existing) = already_downloaded_path(&task, &pid, &tx)
    {
        if cancel.load(Ordering::SeqCst) {
            let _ = tx.send_blocking(DownloadEvent::Cancelled { id: task.id });
            return;
        }
        let _ = tx.send_blocking(DownloadEvent::Skipped {
            id: task.id,
            path: existing.path,
            title: existing.title,
            thumbnail_url: existing.thumbnail_url,
        });
        return;
    }

    let max_attempts = task.retries.max(1).saturating_add(1);
    let mut attempt = 0;

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = tx.send_blocking(DownloadEvent::Cancelled { id: task.id });
            return;
        }

        attempt += 1;
        match run_once(&task, &cancel, &pid, &tx) {
            RunOutcome::Success(path) => {
                let _ = tx.send_blocking(DownloadEvent::Completed {
                    id: task.id,
                    url: compact_url(&task.url),
                    path,
                });
                return;
            }
            RunOutcome::Cancelled => {
                let _ = tx.send_blocking(DownloadEvent::Cancelled { id: task.id });
                return;
            }
            RunOutcome::Failed(error) => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = tx.send_blocking(DownloadEvent::Cancelled { id: task.id });
                    return;
                }
                if attempt >= max_attempts || is_fatal_error(&error) {
                    let _ = tx.send_blocking(DownloadEvent::Failed {
                        id: task.id,
                        url: compact_url(&task.url),
                        error: friendly_error(&error),
                    });
                    return;
                }
                thread::sleep(retry_delay(&error));
            }
        }
    }
}

fn run_once(
    task: &DownloadTask,
    cancel: &AtomicBool,
    pid: &Arc<AtomicU32>,
    tx: &smol::channel::Sender<DownloadEvent>,
) -> RunOutcome {
    if cancel.load(Ordering::SeqCst) {
        return RunOutcome::Cancelled;
    }

    let destination = expand_path(&task.destination);
    if let Err(error) = std::fs::create_dir_all(&destination) {
        return RunOutcome::Failed(format!(
            "保存先を作成できません: {} ({error})",
            destination.display()
        ));
    }

    let Some(yt_dlp) = find_yt_dlp() else {
        return RunOutcome::Failed(
            "yt-dlpが見つかりません。アプリを再起動してツールの準備をやり直してください。"
                .to_string(),
        );
    };

    let converted = convert_template(&task.filename_template);
    let output = destination.join(converted).display().to_string();
    let format = format_spec(task);

    let mut command = Command::new(yt_dlp);
    // 同梱の ffmpeg を使い、PATH 上の別バージョンへ依存しないようにする。
    if let Some(location) = ffmpeg_location() {
        command.arg("--ffmpeg-location").arg(location);
    }
    if let Some(browser) = &task.cookies_from_browser {
        command.arg("--cookies-from-browser").arg(browser);
    }
    apply_single_item_args(&mut command, task);
    apply_overwrite_args(&mut command, task);
    command
        .arg("--ignore-config")
        .arg("--newline")
        .arg("--no-color")
        .arg("--socket-timeout")
        .arg("20")
        .arg("--output-na-placeholder")
        .arg("")
        .arg("--progress-delta")
        .arg("0.5")
        .arg("--progress-template")
        .arg("download:TMD_PROGRESS|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.eta)s")
        .arg("--print")
        .arg("after_move:TMD_METADATA:%(.{title,thumbnail,filepath})j")
        .arg("--no-quiet");

    apply_container_args(&mut command, task.quality, task.audio, task.container);

    command
        .arg("-f")
        .arg(format)
        .arg("-o")
        .arg(output)
        .arg(task.url.as_ref())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return RunOutcome::Failed(format!("yt-dlpを起動できません: {error}"));
        }
    };
    pid.store(child.id(), Ordering::SeqCst);
    let started = Instant::now();
    let last_activity = Arc::new(AtomicU64::new(0));
    let timed_out = Arc::new(AtomicBool::new(false));
    let watchdog_pid = pid.clone();
    let watchdog_target = child.id();
    let watchdog_activity = last_activity.clone();
    let watchdog_timed_out = timed_out.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(500));
            if watchdog_pid.load(Ordering::SeqCst) != watchdog_target {
                return;
            }
            let idle = started
                .elapsed()
                .as_millis()
                .saturating_sub(u128::from(watchdog_activity.load(Ordering::Relaxed)));
            if idle >= IDLE_TIMEOUT.as_millis() {
                watchdog_timed_out.store(true, Ordering::SeqCst);
                let _ = Command::new("/bin/kill")
                    .args(["-TERM", &watchdog_target.to_string()])
                    .status();
                return;
            }
        }
    });

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr_errors = errors.clone();
    let stderr_activity = last_activity.clone();
    let stderr_tx = tx.clone();
    let stderr_thread = stderr.map(|stderr| {
        let task_id = task.id;
        thread::spawn(move || {
            let mut warned = HashSet::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                stderr_activity.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                if line.contains("ERROR:")
                    && let Ok(mut errors) = stderr_errors.lock()
                {
                    errors.push(line.trim().to_string());
                }
                let trimmed = line.trim();
                if trimmed.contains("WARNING:") && warned.insert(trimmed.to_string()) {
                    let _ = stderr_tx.send_blocking(DownloadEvent::Warning {
                        id: task_id,
                        message: trimmed.to_string(),
                    });
                }
            }
        })
    });

    let stdout_thread = stdout.map(|stdout| {
        let stdout_tx = tx.clone();
        let stdout_activity = last_activity.clone();
        let task_id = task.id;
        thread::spawn(move || {
            let mut paths = Vec::new();
            let mut metadata_sent = false;
            let mut last_percent = -1.0_f32;
            let mut last_update = Instant::now();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                stdout_activity.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                let line = line.trim();
                if let Some(mut metadata) = parse_metadata_line(line) {
                    if let Some(path) = output_path_value(metadata.filepath.take()) {
                        paths.push(path);
                    }
                    let title = metadata_value(metadata.title.as_deref());
                    let thumbnail_url = metadata_value(metadata.thumbnail.as_deref());
                    if !metadata_sent && (title.is_some() || thumbnail_url.is_some()) {
                        metadata_sent = true;
                        let _ = stdout_tx.send_blocking(DownloadEvent::Metadata {
                            id: task_id,
                            title,
                            thumbnail_url,
                        });
                    }
                    continue;
                }
                let Some(progress) = parse_progress(line) else {
                    continue;
                };
                let percent = progress.percent();
                if percent < 100.0
                    && (percent - last_percent).abs() < 0.5
                    && last_update.elapsed() < Duration::from_millis(500)
                {
                    continue;
                }
                last_percent = percent;
                last_update = Instant::now();
                let _ = stdout_tx.send_blocking(DownloadEvent::Progress {
                    id: task_id,
                    percent,
                    downloaded: format_bytes(progress.downloaded),
                    total: if progress.total > 0.0 {
                        format_bytes(progress.total)
                    } else {
                        "—".to_string()
                    },
                    eta: progress.eta.map(format_eta),
                });
            }
            paths
        })
    });

    let status = child.wait();
    pid.store(0, Ordering::SeqCst);
    if let Some(handle) = stderr_thread {
        let _ = handle.join();
    }
    let paths = stdout_thread
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    if cancel.load(Ordering::SeqCst) {
        return RunOutcome::Cancelled;
    }
    if timed_out.load(Ordering::SeqCst) {
        return RunOutcome::Failed(format!(
            "yt-dlpが{}秒間出力を返さなかったためタイムアウトしました。ネットワークまたは認証設定を確認してください。",
            IDLE_TIMEOUT.as_secs()
        ));
    }

    let error_text = errors
        .lock()
        .ok()
        .and_then(|errors| errors.last().cloned())
        .unwrap_or_default();

    match status {
        Ok(status) if status.success() => {
            let output_path = match paths.as_slice() {
                [path] => Some(path.clone()),
                _ => None,
            };
            RunOutcome::Success(output_path)
        }
        Ok(status) => {
            if error_text.is_empty() {
                RunOutcome::Failed(format!(
                    "yt-dlpが終了コード {} で終了しました",
                    status.code().unwrap_or(-1)
                ))
            } else {
                RunOutcome::Failed(error_text)
            }
        }
        Err(error) => RunOutcome::Failed(format!("yt-dlpの終了待ちに失敗しました: {error}")),
    }
}

/// 保存先に同じ動画IDのファイルがある場合、そのパスと表示用メタデータを返す。
///
/// IDはyt-dlpのメタデータ取得で確認する。プレイリストやチャンネルのように
/// 複数件へ展開されるURLは対象外とし、通常どおりダウンロードする。
fn already_downloaded_path(
    task: &DownloadTask,
    pid: &AtomicU32,
    tx: &smol::channel::Sender<DownloadEvent>,
) -> Option<ExistingDownload> {
    let destination = expand_path(&task.destination);
    if !destination.is_dir() {
        return None;
    }

    let yt_dlp = find_yt_dlp()?;
    let entries = fetch_video_metadata(task, &yt_dlp, pid, tx)?;
    let [entry] = entries.as_slice() else {
        return None;
    };
    let id = metadata_value(entry.id.as_deref())?;
    let path = find_file_with_id(&destination, &id)?;

    Some(ExistingDownload {
        path: path.display().to_string(),
        title: metadata_value(entry.title.as_deref()),
        thumbnail_url: metadata_value(entry.thumbnail.as_deref()),
    })
}

/// ダウンロード対象のメタデータを、プレイリスト展開を抑えて最大2件取得する。
///
/// 2件以上返った場合は複数件のURLとみなし、ダウンロード済み判定には使わない。
fn fetch_video_metadata(
    task: &DownloadTask,
    yt_dlp: &Path,
    pid: &AtomicU32,
    tx: &smol::channel::Sender<DownloadEvent>,
) -> Option<Vec<PrintedMetadata>> {
    let mut command = Command::new(yt_dlp);
    if let Some(browser) = &task.cookies_from_browser {
        command.arg("--cookies-from-browser").arg(browser);
    }
    if single_video_only(task) {
        command.arg("--no-playlist");
    }

    let child = command
        .arg("--ignore-config")
        .arg("--no-warnings")
        .arg("--no-color")
        .arg("--no-progress")
        .arg("--skip-download")
        .arg("--flat-playlist")
        .arg("--playlist-items")
        .arg(playlist_items_arg(task))
        .arg("--socket-timeout")
        .arg("20")
        .arg("--output-na-placeholder")
        .arg("")
        .arg("--print")
        .arg("video:%(.{id,title,thumbnail})j")
        .arg(task.url.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    pid.store(child.id(), Ordering::SeqCst);
    let output = child.wait_with_output().ok();
    pid.store(0, Ordering::SeqCst);

    let output = output?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (entries, invalid_lines) = parse_probe_metadata(&stdout);
    if invalid_lines > 0 {
        let _ = tx.send_blocking(DownloadEvent::Warning {
            id: task.id,
            message: format!(
                "yt-dlpのメタデータ{invalid_lines}行を解析できませんでした。有効な行だけを確認します。"
            ),
        });
    }
    Some(entries)
}

fn parse_probe_metadata(stdout: &str) -> (Vec<PrintedMetadata>, usize) {
    let mut entries = Vec::new();
    let mut invalid_lines = 0;
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match serde_json::from_str(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => invalid_lines += 1,
        }
    }
    (entries, invalid_lines)
}

fn parse_metadata_line(line: &str) -> Option<PrintedMetadata> {
    serde_json::from_str(line.strip_prefix("TMD_METADATA:")?).ok()
}

fn metadata_value(value: Option<&str>) -> Option<String> {
    crate::model::nonempty_metadata(value).map(str::to_string)
}

fn output_path_value(value: Option<String>) -> Option<String> {
    value.filter(|path| !path.is_empty() && path != "NA")
}

fn find_file_with_id(directory: &Path, id: &str) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path.metadata().is_ok_and(|metadata| metadata.len() > 0)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !is_download_artifact(name) && file_name_has_id(name, id))
        })
}

/// ダウンロード途中の一時ファイルと、動画に付随するファイルは対象外とする。
fn is_download_artifact(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains(".part")
        || name.ends_with(".ytdl")
        || name.contains(".temp")
        || [
            ".description",
            ".json",
            ".jpg",
            ".jpeg",
            ".png",
            ".webp",
            ".vtt",
            ".srt",
            ".ass",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// ファイル名にIDが独立した語として含まれるか。
///
/// タイトルや日付の一部に同じ文字列が含まれるだけのファイルを拾わないよう、
/// IDの前後が英数字以外であることを確認する。
fn file_name_has_id(name: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }

    let mut search_from = 0;
    while let Some(offset) = name[search_from..].find(id) {
        let start = search_from + offset;
        let end = start + id.len();
        let before = name[..start].chars().next_back();
        let after = name[end..].chars().next();
        let bounded_before = before.is_none_or(|ch| !ch.is_alphanumeric());
        let bounded_after = after.is_none_or(|ch| !ch.is_alphanumeric());
        if bounded_before && bounded_after {
            return true;
        }
        search_from = end;
    }

    false
}

struct Progress {
    downloaded: f64,
    total: f64,
    eta: Option<f64>,
}

impl Progress {
    fn percent(&self) -> f32 {
        if self.total > 0.0 {
            ((self.downloaded / self.total) * 100.0).clamp(0.0, 100.0) as f32
        } else {
            0.0
        }
    }
}

fn parse_progress(line: &str) -> Option<Progress> {
    let rest = line.trim().strip_prefix("TMD_PROGRESS|")?;
    let mut parts = rest.split('|');
    let downloaded = parts.next().and_then(parse_number).unwrap_or(0.0);
    let total = parts.next().and_then(parse_number).unwrap_or(0.0);
    let estimate = parts.next().and_then(parse_number).unwrap_or(0.0);
    let eta = parts.next().and_then(parse_number).filter(|eta| *eta > 0.0);
    let total = if total > 0.0 { total } else { estimate };
    Some(Progress {
        downloaded,
        total,
        eta,
    })
}

/// yt-dlpは不明な数値を `NA` で出力するため、数値以外は0として扱う。
fn parse_number(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok()
}

fn format_bytes(value: f64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = value.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_eta(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("残り {hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("残り {minutes:02}:{seconds:02}")
    }
}

fn format_spec(task: &DownloadTask) -> String {
    if task.quality == Quality::AudioOnly {
        return match task.audio {
            AudioQuality::Kbps192 => "ba[abr<=192]/ba/b".to_string(),
            AudioQuality::Kbps128 => "ba[abr<=128]/ba/b".to_string(),
            _ => "ba/b".to_string(),
        };
    }

    let video = match task.quality {
        Quality::Best => "bv*".to_string(),
        Quality::P1080 => "bv*[height<=1080]".to_string(),
        Quality::P720 => "bv*[height<=720]".to_string(),
        Quality::P480 => "bv*[height<=480]".to_string(),
        Quality::AudioOnly => "ba".to_string(),
    };
    let audio = match task.audio {
        AudioQuality::Best => "+ba".to_string(),
        AudioQuality::Kbps192 => "+ba[abr<=192]".to_string(),
        AudioQuality::Kbps128 => "+ba[abr<=128]".to_string(),
        AudioQuality::Mute => String::new(),
    };

    if audio.is_empty() {
        format!("{video}/b")
    } else {
        format!("{video}{audio}/b")
    }
}

/// 画質に応じて、出力コンテナの指定方法を切り替える。
///
/// `映像なし` は音声ファイルとして取り出すため、yt-dlpの音声変換を使う。
/// 音声用コンテナが非可逆形式のときは、選択した音質を変換後のビットレートへ引き継ぐ。
fn apply_container_args(
    command: &mut Command,
    quality: Quality,
    audio: AudioQuality,
    container: Container,
) {
    let container_label = container.label().to_lowercase();
    if quality == Quality::AudioOnly {
        command.arg("-x").arg("--audio-format").arg(container_label);

        let audio_quality = match audio {
            AudioQuality::Best => Some("0"),
            AudioQuality::Kbps192 => Some("192K"),
            AudioQuality::Kbps128 => Some("128K"),
            AudioQuality::Mute => None,
        };
        let is_lossy = matches!(container, Container::M4a | Container::Mp3 | Container::Opus);
        if let Some(audio_quality) = audio_quality.filter(|_| is_lossy) {
            command.arg("--audio-quality").arg(audio_quality);
        }
    } else {
        command
            .arg("--merge-output-format")
            .arg(&container_label)
            .arg("--remux-video")
            .arg(&container_label);
    }
}

fn convert_template(template: &str) -> String {
    let mut converted = template.to_string();
    for (custom, yt_dlp) in [
        ("uploadDate", "upload_date"),
        ("title", "title"),
        ("duration", "duration>%Hh%Mm%Ss"),
        ("width", "width"),
        ("height", "height"),
        ("vcodec", "vcodec"),
        ("fps", "fps"),
        ("acodec", "acodec"),
        ("abr", "abr"),
        ("id", "id"),
        ("ext", "ext"),
    ] {
        converted = converted.replace(&format!("${{{custom}}}"), &format!("%({yt_dlp})s"));
    }
    if !converted.contains("%(ext)s") {
        converted.push_str(".%(ext)s");
    }
    converted
}

/// アプリが管理する yt-dlp を最優先で使う。
///
/// `YT_DLP_PATH` は開発時の差し替え用として残す。管理下のバイナリが無い場合だけ
/// 従来の探索へフォールバックし、起動時の準備が完了していない状態でも動作を確認できる。
fn find_yt_dlp() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("YT_DLP_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    let managed = ToolPaths::from_env().yt_dlp();
    if managed.is_file() {
        return Some(managed);
    }

    let local = Path::new("bin").join("yt-dlp");
    if local.is_file() {
        return Some(local);
    }

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("yt-dlp");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    for candidate in ["/opt/homebrew/bin/yt-dlp", "/usr/local/bin/yt-dlp"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// yt-dlp へ `--ffmpeg-location` で渡す、管理下の ffmpeg ディレクトリ。
fn ffmpeg_location() -> Option<PathBuf> {
    let paths = ToolPaths::from_env();
    (paths.ffmpeg().is_file() && paths.ffprobe().is_file()).then(|| paths.ffmpeg_dir())
}

/// 再試行しても改善しないエラー。
fn is_fatal_error(error: &str) -> bool {
    [
        "Sign in to confirm",
        "Video unavailable",
        "This video is not available",
        "Private video",
        "members-only",
        "Requested format is not available",
        "Unsupported URL",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

/// レート制限は待機時間を長めにして、設定された回数まで再試行する。
fn retry_delay(error: &str) -> Duration {
    if error.contains("429") || error.contains("Too Many Requests") {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    }
}

fn friendly_error(error: &str) -> String {
    if error.contains("Sign in to confirm") || error.contains("not a bot") {
        format!(
            "{error}\nYouTubeの認証が必要です。設定画面の「ブラウザのCookie」でログイン済みのブラウザを選ぶか、環境変数 YT_DLP_COOKIES_FROM_BROWSER（例: chrome / safari）を設定してください。"
        )
    } else if error.contains("429") || error.contains("Too Many Requests") {
        format!("{error}\nYouTubeのレート制限です。時間をおいて再試行してください。")
    } else {
        error.to_string()
    }
}

struct TerminalFeedback {
    success: bool,
    notification: Option<String>,
}

fn terminal_feedback(event: &DownloadEvent) -> Option<TerminalFeedback> {
    match event {
        DownloadEvent::Completed { url, .. } => Some(TerminalFeedback {
            success: true,
            notification: Some(format!("ダウンロード完了: {url}")),
        }),
        DownloadEvent::Failed { url, .. } => Some(TerminalFeedback {
            success: false,
            notification: Some(format!("ダウンロード失敗: {url}")),
        }),
        DownloadEvent::Skipped { .. } => Some(TerminalFeedback {
            success: true,
            notification: None,
        }),
        DownloadEvent::Progress { .. }
        | DownloadEvent::Metadata { .. }
        | DownloadEvent::Warning { .. }
        | DownloadEvent::Cancelled { .. } => None,
    }
}

fn report_terminal_feedback(feedback: Option<TerminalFeedback>) {
    let Some(feedback) = feedback else {
        return;
    };
    let settings = AppSettings::load();

    if settings.notify_on_complete
        && let Some(message) = feedback.notification
    {
        let script = format!(
            "display notification \"{}\" with title \"Tree Movie Downloader\"",
            escape_applescript(&message)
        );
        let _ = Command::new("osascript").arg("-e").arg(script).spawn();
    }

    if settings.play_sound {
        let kind = if feedback.success { "success" } else { "error" };
        let (selected, random) = if feedback.success {
            (settings.success_sound.as_deref(), settings.success_random)
        } else {
            (settings.error_sound.as_deref(), settings.error_random)
        };
        let options = scan_sounds(kind);
        let sound = if random {
            if options.is_empty() {
                None
            } else {
                let index = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.subsec_nanos() as usize % options.len())
                    .unwrap_or(0);
                Some(options[index].path.clone())
            }
        } else {
            selected
                .and_then(|id| options.iter().find(|option| option.id == id))
                .map(|option| option.path.clone())
                .or_else(|| options.first().map(|option| option.path.clone()))
        };
        if let Some(path) = sound {
            let _ = play_sound(&path);
        }
    }
}

fn escape_applescript(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> DownloadTask {
        DownloadTask {
            id: 1,
            url: "https://example.com/video".into(),
            quality: Quality::P1080,
            audio: AudioQuality::Best,
            container: Container::Mkv,
            destination: "/tmp".into(),
            filename_template: "${uploadDate}_${title}.${ext}".into(),
            cookies_from_browser: None,
            retries: 1,
            single_item: false,
            force: false,
        }
    }

    #[test]
    fn skipped_items_use_success_completion_feedback() {
        let feedback = terminal_feedback(&DownloadEvent::Skipped {
            id: 1,
            path: "/tmp/video.mp4".into(),
            title: None,
            thumbnail_url: None,
        })
        .expect("skipped items are terminal");

        assert!(feedback.success);
        assert_eq!(feedback.notification, None);
    }

    #[test]
    fn completed_and_failed_items_keep_their_completion_feedback() {
        let completed = terminal_feedback(&DownloadEvent::Completed {
            id: 1,
            url: "https://example.com/video".into(),
            path: Some("/tmp/video.mp4".into()),
        })
        .expect("completed items are terminal");
        assert!(completed.success);
        assert_eq!(
            completed.notification.as_deref(),
            Some("ダウンロード完了: https://example.com/video")
        );

        let failed = terminal_feedback(&DownloadEvent::Failed {
            id: 2,
            url: "https://example.com/other".into(),
            error: "failed".into(),
        })
        .expect("failed items are terminal");
        assert!(!failed.success);
        assert_eq!(
            failed.notification.as_deref(),
            Some("ダウンロード失敗: https://example.com/other")
        );
    }

    #[test]
    fn non_terminal_events_do_not_report_completion_feedback() {
        let events = [
            DownloadEvent::Progress {
                id: 1,
                percent: 50.0,
                downloaded: "1 MB".into(),
                total: "2 MB".into(),
                eta: Some("10".into()),
            },
            DownloadEvent::Warning {
                id: 1,
                message: "warning".into(),
            },
            DownloadEvent::Metadata {
                id: 1,
                title: Some("title".into()),
                thumbnail_url: None,
            },
            DownloadEvent::Cancelled { id: 1 },
        ];

        assert!(
            events
                .iter()
                .all(|event| terminal_feedback(event).is_none())
        );
    }

    #[test]
    fn converts_custom_template_to_yt_dlp_fields() {
        assert_eq!(
            convert_template("${uploadDate}_${title} [${duration}].${ext}"),
            "%(upload_date)s_%(title)s [%(duration>%Hh%Mm%Ss)s].%(ext)s"
        );
        assert_eq!(convert_template("${title}"), "%(title)s.%(ext)s");
    }

    #[test]
    fn builds_format_selector() {
        let mut task = task();
        assert_eq!(format_spec(&task), "bv*[height<=1080]+ba/b");

        task.quality = Quality::AudioOnly;
        assert_eq!(format_spec(&task), "ba/b");

        task.quality = Quality::P720;
        task.audio = AudioQuality::Mute;
        assert_eq!(format_spec(&task), "bv*[height<=720]/b");
    }

    #[test]
    fn forced_redownloads_overwrite_existing_files() {
        fn args(command: &Command) -> Vec<String> {
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        }

        let mut task = task();
        let mut command = Command::new("yt-dlp");
        apply_overwrite_args(&mut command, &task);
        assert_eq!(args(&command), vec!["--no-overwrites"]);

        task.force = true;
        let mut command = Command::new("yt-dlp");
        apply_overwrite_args(&mut command, &task);
        assert_eq!(args(&command), vec!["--force-overwrites"]);
    }

    #[test]
    fn builds_container_options_for_audio_and_video() {
        fn args(command: &Command) -> Vec<String> {
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        }

        let mut command = Command::new("yt-dlp");
        apply_container_args(
            &mut command,
            Quality::AudioOnly,
            AudioQuality::Best,
            Container::M4a,
        );
        assert_eq!(
            args(&command),
            vec!["-x", "--audio-format", "m4a", "--audio-quality", "0"]
        );

        let mut command = Command::new("yt-dlp");
        apply_container_args(
            &mut command,
            Quality::AudioOnly,
            AudioQuality::Kbps128,
            Container::Flac,
        );
        assert_eq!(args(&command), vec!["-x", "--audio-format", "flac"]);

        let mut command = Command::new("yt-dlp");
        apply_container_args(
            &mut command,
            Quality::P1080,
            AudioQuality::Best,
            Container::Webm,
        );
        assert_eq!(
            args(&command),
            vec!["--merge-output-format", "webm", "--remux-video", "webm"]
        );
    }

    #[test]
    fn disables_playlists_for_video_urls_with_a_list_parameter() {
        assert!(is_video_with_playlist(
            "https://youtu.be/OHuNxItVx_A?list=RDOHuNxItVx_A"
        ));
        assert!(is_video_with_playlist(
            "https://www.youtube.com/watch?v=OHuNxItVx_A&list=RDOHuNxItVx_A"
        ));
        assert!(is_video_with_playlist(
            "https://www.youtube.com/shorts/abc?list=PL1"
        ));

        assert!(!is_video_with_playlist(
            "https://www.youtube.com/playlist?list=PL1"
        ));
        assert!(!is_video_with_playlist(
            "https://www.youtube.com/@channel/videos"
        ));
        assert!(!is_video_with_playlist(
            "https://www.youtube.com/watch?v=OHuNxItVx_A"
        ));
    }

    #[test]
    fn single_item_downloads_are_limited_to_one_playlist_entry() {
        fn args(command: &Command) -> Vec<String> {
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        }

        let mut task = task();
        task.single_item = true;
        let mut command = Command::new("yt-dlp");
        apply_single_item_args(&mut command, &task);
        assert_eq!(
            args(&command),
            vec!["--no-playlist", "--playlist-items", "1"]
        );

        task.single_item = false;
        let mut command = Command::new("yt-dlp");
        apply_single_item_args(&mut command, &task);
        assert!(args(&command).is_empty());
    }

    #[test]
    fn parses_progress_template_line() {
        let progress = parse_progress("TMD_PROGRESS|1024|4096|0|12").unwrap();
        assert!((progress.percent() - 25.0).abs() < f32::EPSILON);
        assert_eq!(progress.downloaded, 1024.0);
        assert_eq!(progress.total, 4096.0);
        assert_eq!(progress.eta, Some(12.0));
    }

    #[test]
    fn uses_the_size_estimate_when_the_total_is_unknown() {
        let progress = parse_progress("TMD_PROGRESS|1024|NA|4096|12").unwrap();
        assert!((progress.percent() - 25.0).abs() < f32::EPSILON);
        assert_eq!(progress.total, 4096.0);
    }

    #[test]
    fn ignores_unknown_progress_values() {
        let progress = parse_progress("TMD_PROGRESS|NA|NA|NA|NA").unwrap();
        assert_eq!(progress.percent(), 0.0);

        assert!(parse_progress("[download] Destination: /tmp/video.mp4").is_none());
    }

    #[test]
    fn treats_authentication_errors_as_fatal() {
        assert!(is_fatal_error("ERROR: Sign in to confirm you’re not a bot"));
        assert!(is_fatal_error("ERROR: Private video"));
        assert!(!is_fatal_error("ERROR: HTTP Error 429: Too Many Requests"));
        assert!(!is_fatal_error(
            "yt-dlpが180秒間出力を返さなかったためタイムアウトしました"
        ));
    }

    #[test]
    fn rate_limit_waits_longer_before_retrying() {
        assert_eq!(
            retry_delay("ERROR: HTTP Error 429: Too Many Requests"),
            Duration::from_secs(15)
        );
        assert_eq!(
            retry_delay("ERROR: unable to download video data"),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn parses_video_ids_from_print_output() {
        let (metadata, invalid_lines) =
            parse_probe_metadata("{\"id\":\"abc123\"}\n\n{\"id\":\" def456 \"}\n{\"id\":\"NA\"}\n");
        assert_eq!(invalid_lines, 0);
        let ids: Vec<_> = metadata
            .iter()
            .filter_map(|entry| metadata_value(entry.id.as_deref()))
            .collect();
        assert_eq!(ids, vec!["abc123", "def456"]);
    }

    #[test]
    fn parses_video_metadata_from_json_print_output() {
        let stdout = concat!(
            "{\"id\":\"abc123\",\"title\":\"日本語 \\\"タイトル\\\"\",\"thumbnail\":\"https://example.com/thumb.jpg\"}\n",
            "{\"id\":\"def456\",\"title\":\"\",\"thumbnail\":\"NA\"}\n",
        );
        let (metadata, invalid_lines) = parse_probe_metadata(stdout);
        assert_eq!(invalid_lines, 0);
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].id.as_deref(), Some("abc123"));
        assert_eq!(metadata[0].title.as_deref(), Some("日本語 \"タイトル\""));
        assert_eq!(
            metadata[0].thumbnail.as_deref(),
            Some("https://example.com/thumb.jpg")
        );
        let (metadata, invalid_lines) = parse_probe_metadata("{\"id\":\"abc123\"}\nnot-json");
        assert_eq!(metadata.len(), 1);
        assert_eq!(invalid_lines, 1);
    }

    #[test]
    fn parses_after_move_metadata_without_losing_escaped_text() {
        let metadata = parse_metadata_line(
            "TMD_METADATA:{\"title\":\"1行目\\n2行目\",\"thumbnail\":\"https://example.com/thumb.jpg\",\"filepath\":\"/tmp/video.mp4\"}",
        )
        .expect("parse metadata line");

        assert_eq!(metadata.title.as_deref(), Some("1行目\n2行目"));
        assert_eq!(
            metadata.thumbnail.as_deref(),
            Some("https://example.com/thumb.jpg")
        );
        assert_eq!(metadata.filepath.as_deref(), Some("/tmp/video.mp4"));
        assert!(parse_metadata_line("TMD_METADATA:not-json").is_none());
        assert!(parse_metadata_line("[download] Destination: /tmp/video.mp4").is_none());
    }

    #[test]
    fn ignores_empty_metadata_values() {
        assert_eq!(metadata_value(Some("  ")), None);
        assert_eq!(metadata_value(Some("NA")), None);
        assert_eq!(
            metadata_value(Some(" タイトル ")).as_deref(),
            Some("タイトル")
        );
        assert_eq!(
            output_path_value(Some(" /tmp/video.mkv ".into())).as_deref(),
            Some(" /tmp/video.mkv ")
        );
        assert_eq!(output_path_value(Some("  ".into())).as_deref(), Some("  "));
        assert_eq!(output_path_value(Some(String::new())), None);
        assert_eq!(output_path_value(Some("NA".into())), None);
    }

    #[test]
    fn detects_ids_as_separate_filename_tokens() {
        assert!(file_name_has_id(
            "20260907_title [abc123][1080p].mkv",
            "abc123"
        ));
        assert!(file_name_has_id("title_abc123.mp4", "abc123"));
        assert!(file_name_has_id("abc123.mkv", "abc123"));
        assert!(file_name_has_id("title [ZqSsDmJI-3s].webm", "ZqSsDmJI-3s"));

        assert!(!file_name_has_id("title_abc1234.mp4", "abc123"));
        assert!(!file_name_has_id("title_def456.mp4", "abc123"));
        assert!(!file_name_has_id("title.mp4", ""));
    }

    #[test]
    fn ignores_partial_and_sidecar_files() {
        assert!(is_download_artifact("title [abc123].mp4.part"));
        assert!(is_download_artifact("title [abc123].mp4.ytdl"));
        assert!(is_download_artifact("title [abc123].info.json"));
        assert!(is_download_artifact("title [abc123].webp"));

        assert!(!is_download_artifact("title [abc123].mp4"));
        assert!(!is_download_artifact("title [abc123].webm"));
    }

    #[test]
    fn finds_a_downloaded_media_file_by_id() {
        let directory =
            std::env::temp_dir().join(format!("tmd-existing-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create temp dir");

        std::fs::write(directory.join("title [abc123].mp4.part"), b"").expect("write part file");
        std::fs::write(directory.join("title [abc123].info.json"), b"").expect("write sidecar");
        std::fs::write(directory.join("empty [def456].mp4"), b"").expect("write empty file");
        assert!(find_file_with_id(&directory, "abc123").is_none());
        assert!(find_file_with_id(&directory, "def456").is_none());

        let media = directory.join("title [abc123].mp4");
        std::fs::write(&media, b"video").expect("write media file");
        assert_eq!(find_file_with_id(&directory, "abc123"), Some(media));
        assert!(find_file_with_id(&directory, "abc1234").is_none());

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn formats_eta_and_bytes() {
        assert_eq!(format_eta(12.0), "残り 00:12");
        assert_eq!(format_eta(3723.0), "残り 01:02:03");
        assert_eq!(format_bytes(1024.0), "1.0 KB");
    }
}
