//! キューとダウンロード設定のモデル。
//!
//! この段階では yt-dlp を呼び出さず、画面確認のための状態だけを持つ。

use std::path::PathBuf;

use chrono::Local;
use gpui_kit::{EventEmitter, SharedString};

/// ダウンロードする画質。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Best,
    P1080,
    P720,
    P480,
    AudioOnly,
}

impl Quality {
    pub const ALL: [Self; 5] = [
        Self::Best,
        Self::P1080,
        Self::P720,
        Self::P480,
        Self::AudioOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Best => "最高画質（自動）",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::AudioOnly => "映像なし",
        }
    }

    /// 表やステータスバーに出す短い表記。
    pub fn short(self) -> &'static str {
        match self {
            Self::Best => "自動",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::AudioOnly => "映像なし",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|quality| *quality == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::P1080)
    }
}

/// 音質。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioQuality {
    Best,
    Kbps192,
    Kbps128,
    Mute,
}

impl AudioQuality {
    pub const ALL: [Self; 4] = [Self::Best, Self::Kbps192, Self::Kbps128, Self::Mute];

    pub fn label(self) -> &'static str {
        match self {
            Self::Best => "最高音質",
            Self::Kbps192 => "192 kbps",
            Self::Kbps128 => "128 kbps",
            Self::Mute => "音声なし",
        }
    }

    /// 表のメニューボタンに出す短い表記。
    pub fn short(self) -> &'static str {
        match self {
            Self::Best => "最高",
            Self::Kbps192 => "192k",
            Self::Kbps128 => "128k",
            Self::Mute => "なし",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|audio| *audio == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Best)
    }
}

/// 出力コンテナ。
///
/// 映像ありの画質では映像用コンテナ、`映像なし` では音声用コンテナを使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Mkv,
    Webm,
    M4a,
    Mp3,
    Opus,
    Flac,
}

impl Container {
    /// 映像用のコンテナ。
    pub const VIDEO: [Self; 3] = [Self::Mp4, Self::Mkv, Self::Webm];
    /// 音声用のコンテナ。
    pub const AUDIO: [Self; 4] = [Self::M4a, Self::Mp3, Self::Opus, Self::Flac];
    /// 設定画面など、すべてのコンテナを並べるときの順序。
    pub const ALL: [Self; 7] = [
        Self::Mp4,
        Self::Mkv,
        Self::Webm,
        Self::M4a,
        Self::Mp3,
        Self::Opus,
        Self::Flac,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "WebM",
            Self::M4a => "m4a",
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Flac => "flac",
        }
    }

    /// 音声用コンテナかどうか。
    pub fn is_audio(self) -> bool {
        Self::AUDIO.contains(&self)
    }

    /// 画質に応じたコンテナの選択肢。
    pub fn options_for(quality: Quality) -> &'static [Self] {
        if quality == Quality::AudioOnly {
            &Self::AUDIO
        } else {
            &Self::VIDEO
        }
    }

    /// 画質とコンテナの種類が食い違う場合、画質に合う既定値へ置き換える。
    pub fn for_quality(self, quality: Quality) -> Self {
        let wants_audio = quality == Quality::AudioOnly;
        if wants_audio == self.is_audio() {
            self
        } else if wants_audio {
            DEFAULT_AUDIO_CONTAINER
        } else {
            DEFAULT_CONTAINER
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|container| *container == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Mkv)
    }
}

/// URLを追加したときの画質の初期値。
pub const DEFAULT_QUALITY: Quality = Quality::P1080;
/// URLを追加したときの音質の初期値。
pub const DEFAULT_AUDIO: AudioQuality = AudioQuality::Best;
/// 映像用コンテナの初期値。
pub const DEFAULT_CONTAINER: Container = Container::Mkv;
/// `映像なし` を選んだときの音声用コンテナの初期値。
pub const DEFAULT_AUDIO_CONTAINER: Container = Container::M4a;

/// 自動リトライ回数の選択肢。
pub const RETRY_CHOICES: [usize; 3] = [1, 3, 5];
pub const RETRY_LABELS: [&str; 3] = ["1回", "3回", "5回"];

/// キュー全体に適用するダウンロード設定。
///
/// 画質と音質はURLごとに保持するため、ここには含めない。
#[derive(Clone, Copy, Debug)]
pub struct DownloadSettings {
    container: Container,
    retries: usize,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            container: DEFAULT_CONTAINER,
            retries: 3,
        }
    }
}

impl DownloadSettings {
    pub fn container(&self) -> Container {
        self.container
    }

    pub fn set_container(&mut self, container: Container) {
        self.container = container;
    }

    pub fn retries(&self) -> usize {
        self.retries
    }

    pub fn set_retries(&mut self, retries: usize) {
        self.retries = retries;
    }

    pub fn retry_index(&self) -> usize {
        RETRY_CHOICES
            .iter()
            .position(|choice| *choice == self.retries)
            .unwrap_or(1)
    }
}

/// URLの登録元サイト。サイト単位でキューを分け、同時実行数を1件に制限する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Site {
    YouTube,
    X,
    Twitch,
    Other,
}

impl Site {
    pub const ALL: [Self; 4] = [Self::YouTube, Self::X, Self::Twitch, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::X => "X",
            Self::Twitch => "Twitch",
            Self::Other => "その他",
        }
    }

    pub fn from_url(url: &str) -> Self {
        let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
        let host = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim_start_matches("www.")
            .to_lowercase();

        if host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com") {
            Self::YouTube
        } else if host == "x.com"
            || host.ends_with(".x.com")
            || host == "twitter.com"
            || host.ends_with(".twitter.com")
        {
            Self::X
        } else if host == "twitch.tv" || host.ends_with(".twitch.tv") {
            Self::Twitch
        } else {
            Self::Other
        }
    }
}

/// OS標準のダウンロードフォルダ。
pub fn default_downloads_dir() -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Downloads").display().to_string()
}

/// キューの1項目の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Skipped,
}

impl ItemStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "待機中",
            Self::Downloading => "取得中",
            Self::Paused => "一時停止",
            Self::Completed => "完了",
            Self::Failed => "失敗",
            Self::Skipped => "スキップ",
        }
    }
}

/// キューの1項目。
///
/// タイトルはYouTube APIなしでは取得できないため保持せず、URLを表示上の識別子とする。
#[derive(Clone)]
pub struct QueueItem {
    id: u64,
    url: SharedString,
    site: Site,
    destination: SharedString,
    status: ItemStatus,
    progress: f32,
    downloaded: SharedString,
    total: SharedString,
    eta: Option<SharedString>,
    quality: Quality,
    audio: AudioQuality,
    container: Container,
    attempts: usize,
    error: Option<SharedString>,
    output_path: Option<SharedString>,
}

impl QueueItem {
    fn new(
        id: u64,
        url: impl Into<SharedString>,
        quality: Quality,
        audio: AudioQuality,
        container: Container,
        destination: impl Into<SharedString>,
    ) -> Self {
        let url = url.into();
        let site = Site::from_url(&url);
        Self {
            id,
            url,
            site,
            destination: destination.into(),
            status: ItemStatus::Queued,
            progress: 0.0,
            downloaded: "—".into(),
            total: "—".into(),
            eta: None,
            quality,
            audio,
            container: container.for_quality(quality),
            attempts: 0,
            error: None,
            output_path: None,
        }
    }

    #[cfg(test)]
    fn with_status(
        mut self,
        status: ItemStatus,
        progress: f32,
        downloaded: impl Into<SharedString>,
        total: impl Into<SharedString>,
    ) -> Self {
        self.status = status;
        self.progress = progress;
        self.downloaded = downloaded.into();
        self.total = total.into();
        self
    }

    #[cfg(test)]
    fn with_error(mut self, error: impl Into<SharedString>, attempts: usize) -> Self {
        self.error = Some(error.into());
        self.attempts = attempts;
        self
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn url(&self) -> &SharedString {
        &self.url
    }

    pub fn site(&self) -> Site {
        self.site
    }

    pub fn destination(&self) -> &SharedString {
        &self.destination
    }

    pub fn status(&self) -> ItemStatus {
        self.status
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn downloaded(&self) -> &SharedString {
        &self.downloaded
    }

    pub fn total(&self) -> &SharedString {
        &self.total
    }

    pub fn eta(&self) -> Option<&SharedString> {
        self.eta.as_ref()
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    pub fn audio(&self) -> AudioQuality {
        self.audio
    }

    pub fn container(&self) -> Container {
        self.container
    }

    pub fn attempts(&self) -> usize {
        self.attempts
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn output_path(&self) -> Option<&str> {
        if matches!(self.status, ItemStatus::Completed | ItemStatus::Skipped) {
            self.output_path.as_deref()
        } else {
            None
        }
    }
}

/// ログの重要度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// ログパネルの1行。
#[derive(Clone)]
pub struct LogEntry {
    time: SharedString,
    level: LogLevel,
    site: Option<Site>,
    message: SharedString,
}

impl LogEntry {
    pub fn time(&self) -> &SharedString {
        &self.time
    }

    pub fn level(&self) -> LogLevel {
        self.level
    }

    pub fn message(&self) -> &SharedString {
        &self.message
    }
}

/// キューの変更を他のビューへ知らせるイベント。
#[derive(Clone, Copy, Debug)]
pub enum QueueEvent {
    Changed,
}

const LOG_LIMIT: usize = 200;
const LOG_URL_LIMIT: usize = 64;

/// 表に出すためのURL表記。スキームと `www.` を省いて識別しやすくする。
pub fn display_url(url: &str) -> String {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    stripped
        .strip_prefix("www.")
        .unwrap_or(stripped)
        .to_string()
}

/// ログに出すための短いURL表記。
pub fn compact_url(url: &str) -> String {
    let stripped = display_url(url);
    if stripped.chars().count() > LOG_URL_LIMIT {
        let mut text: String = stripped.chars().take(LOG_URL_LIMIT).collect();
        text.push('…');
        text
    } else {
        stripped
    }
}

/// キュー、設定、ログをまとめて保持する。
pub struct QueueModel {
    items: Vec<QueueItem>,
    settings: DownloadSettings,
    destinations: Vec<String>,
    logs: Vec<LogEntry>,
    next_id: u64,
}

impl EventEmitter<QueueEvent> for QueueModel {}

impl Default for QueueModel {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueModel {
    /// 空のキューを作る。起動直後はこの状態から始める。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            settings: DownloadSettings::default(),
            destinations: vec![default_downloads_dir()],
            logs: Vec::new(),
            next_id: 1,
        }
    }

    /// テストでキューとログの状態を再現するためのダミーデータ。
    #[cfg(test)]
    pub fn demo() -> Self {
        let items = vec![
            QueueItem::new(
                1,
                "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
                Quality::P1080,
                AudioQuality::Best,
                Container::Mkv,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Completed, 100.0, "1.2 GB", "1.2 GB"),
            QueueItem::new(
                2,
                "https://www.youtube.com/watch?v=8jPQjjsBbIc",
                Quality::P1080,
                AudioQuality::Best,
                Container::Mp4,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Completed, 100.0, "1.1 GB", "1.1 GB"),
            QueueItem::new(
                3,
                "https://www.youtube.com/@BlenderStudio",
                Quality::P720,
                AudioQuality::Best,
                Container::Mkv,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Skipped, 0.0, "—", "—"),
            QueueItem::new(
                4,
                "https://x.com/example/status/1840000000000000000",
                Quality::P1080,
                AudioQuality::Best,
                Container::Mp4,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Completed, 100.0, "820 MB", "820 MB"),
            QueueItem::new(
                5,
                "https://x.com/example/status/1840000000000000001",
                Quality::P720,
                AudioQuality::Best,
                Container::Mkv,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Skipped, 0.0, "—", "—"),
            QueueItem::new(
                6,
                "https://www.twitch.tv/videos/2233445566",
                Quality::P720,
                AudioQuality::Kbps128,
                Container::Mkv,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Completed, 100.0, "340 MB", "340 MB"),
            QueueItem::new(
                7,
                "https://www.twitch.tv/example_channel",
                Quality::P720,
                AudioQuality::Best,
                Container::Mkv,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Completed, 100.0, "1.9 GB", "1.9 GB"),
            QueueItem::new(
                8,
                "https://vimeo.com/987654321",
                Quality::P1080,
                AudioQuality::Kbps192,
                Container::Mp4,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Failed, 38.0, "512 MB", "1.3 GB")
            .with_error("HTTP 429: Too Many Requests", 3),
            QueueItem::new(
                9,
                "https://www.dailymotion.com/video/x123",
                Quality::AudioOnly,
                AudioQuality::Best,
                Container::Webm,
                default_downloads_dir(),
            )
            .with_status(ItemStatus::Skipped, 0.0, "—", "—"),
        ];

        let mut model = Self {
            items,
            settings: DownloadSettings::default(),
            destinations: vec![default_downloads_dir()],
            logs: Vec::new(),
            next_id: 10,
        };
        model.demo_logs();
        model
    }

    #[cfg(test)]
    fn demo_logs(&mut self) {
        let logs = [
            (
                "12:34:56",
                LogLevel::Info,
                "キューを開始しました（サイトごとに1件ずつ処理）",
            ),
            ("12:34:57", LogLevel::Info, "[1/7] aqz-KE-bpKQ の取得を開始"),
            (
                "12:35:02",
                LogLevel::Info,
                "[YouTube] 1080p / mkv / 最高音質 を選択しました",
            ),
            (
                "12:35:06",
                LogLevel::Info,
                "[X] 1080p / mp4 / 最高音質 を選択しました",
            ),
            (
                "12:35:10",
                LogLevel::Warn,
                "[YouTube] レート制限を検出しました。10秒後に再試行します（1/3）",
            ),
            ("12:35:20", LogLevel::Info, "[1/7] 再開しました"),
            (
                "12:35:28",
                LogLevel::Error,
                "[Vimeo] HTTP 429: Too Many Requests",
            ),
            (
                "12:35:29",
                LogLevel::Info,
                "[Vimeo] リトライ回数の上限に達したため、次の項目へ進みます",
            ),
        ];

        self.logs = logs
            .into_iter()
            .map(|(time, level, message)| {
                let site = if message.contains("[YouTube]") {
                    Some(Site::YouTube)
                } else if message.contains("[X]") {
                    Some(Site::X)
                } else if message.contains("[Twitch]") {
                    Some(Site::Twitch)
                } else if message.contains("[Vimeo]") {
                    Some(Site::Other)
                } else {
                    None
                };
                LogEntry {
                    time: time.into(),
                    level,
                    site,
                    message: message.into(),
                }
            })
            .collect();
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    pub fn settings(&self) -> &DownloadSettings {
        &self.settings
    }

    pub fn destinations(&self) -> &[String] {
        &self.destinations
    }

    pub fn default_destination(&self) -> String {
        self.destinations
            .first()
            .cloned()
            .unwrap_or_else(default_downloads_dir)
    }

    pub fn set_destinations(&mut self, destinations: Vec<String>) {
        let mut normalized = vec![default_downloads_dir()];
        for destination in destinations {
            let destination = destination.trim().to_string();
            if !destination.is_empty() && !normalized.contains(&destination) {
                normalized.push(destination);
            }
        }
        self.destinations = normalized;
    }

    pub fn set_container(&mut self, container: Container) {
        self.settings.set_container(container);
    }

    pub fn set_retries(&mut self, retries: usize) {
        self.settings.set_retries(retries);
    }

    /// URLごとの画質を変更する。開始済みの項目は変更しない。
    pub fn set_item_quality(&mut self, id: u64, quality: Quality) {
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.id == id && item.status == ItemStatus::Queued)
        {
            item.quality = quality;
            item.container = item.container.for_quality(quality);
        }
    }

    /// URLごとの音質を変更する。開始済みの項目は変更しない。
    pub fn set_item_audio(&mut self, id: u64, audio: AudioQuality) {
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.id == id && item.status == ItemStatus::Queued)
        {
            item.audio = audio;
        }
    }

    /// URLごとのコンテナを変更する。開始済みの項目は変更しない。
    pub fn set_item_container(&mut self, id: u64, container: Container) {
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.id == id && item.status == ItemStatus::Queued)
        {
            item.container = container.for_quality(item.quality);
        }
    }

    /// URLごとの保存先を変更する。開始済みの項目は変更しない。
    pub fn set_item_destination(&mut self, id: u64, destination: &str) {
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.id == id && item.status == ItemStatus::Queued)
        {
            item.destination = destination.to_string().into();
        }
    }

    pub fn set_all_quality_for_site(&mut self, site: Site, quality: Quality) {
        for item in self
            .items
            .iter_mut()
            .filter(|item| item.site == site && item.status == ItemStatus::Queued)
        {
            item.quality = quality;
            item.container = item.container.for_quality(quality);
        }
    }

    pub fn set_all_audio_for_site(&mut self, site: Site, audio: AudioQuality) {
        for item in self
            .items
            .iter_mut()
            .filter(|item| item.site == site && item.status == ItemStatus::Queued)
        {
            item.audio = audio;
        }
    }

    pub fn set_all_container_for_site(&mut self, site: Site, container: Container) {
        for item in self
            .items
            .iter_mut()
            .filter(|item| item.site == site && item.status == ItemStatus::Queued)
        {
            item.container = container.for_quality(item.quality);
        }
    }

    pub fn set_all_destination_for_site(&mut self, site: Site, destination: &str) {
        for item in self
            .items
            .iter_mut()
            .filter(|item| item.site == site && item.status == ItemStatus::Queued)
        {
            item.destination = destination.to_string().into();
        }
    }

    pub fn push_log(&mut self, level: LogLevel, message: impl Into<SharedString>) {
        self.push_log_entry(None, level, message);
    }

    pub fn push_log_for_site(
        &mut self,
        site: Site,
        level: LogLevel,
        message: impl Into<SharedString>,
    ) {
        self.push_log_entry(Some(site), level, message);
    }

    fn push_log_entry(
        &mut self,
        site: Option<Site>,
        level: LogLevel,
        message: impl Into<SharedString>,
    ) {
        let time = Local::now().format("%H:%M:%S").to_string();
        self.logs.push(LogEntry {
            time: time.into(),
            level,
            site,
            message: message.into(),
        });
        if self.logs.len() > LOG_LIMIT {
            let overflow = self.logs.len() - LOG_LIMIT;
            self.logs.drain(0..overflow);
        }
    }

    pub fn logs_for_site(&self, site: Site) -> Vec<LogEntry> {
        self.logs
            .iter()
            .filter(|entry| entry.site == Some(site))
            .cloned()
            .collect()
    }

    /// 指定サイトのログを消す。消した件数を返す。
    pub fn clear_logs_for_site(&mut self, site: Site) -> usize {
        let before = self.logs.len();
        self.logs.retain(|entry| entry.site != Some(site));
        before - self.logs.len()
    }

    pub fn completed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == ItemStatus::Completed)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == ItemStatus::Failed)
            .count()
    }

    pub fn overall_progress(&self) -> f32 {
        if self.items.is_empty() {
            return 0.0;
        }

        let sum: f32 = self
            .items
            .iter()
            .map(|item| match item.status {
                ItemStatus::Completed => 100.0,
                ItemStatus::Skipped => 0.0,
                _ => item.progress,
            })
            .sum();
        sum / self.items.len() as f32
    }

    pub fn active_item(&self) -> Option<&QueueItem> {
        self.items
            .iter()
            .find(|item| item.status == ItemStatus::Downloading)
    }

    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == ItemStatus::Downloading)
            .count()
    }

    /// 開始待ち（待機中・一時停止）の項目があるか。
    pub fn has_pending(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item.status, ItemStatus::Queued | ItemStatus::Paused))
    }

    /// キュー内のすべての項目が終了状態（完了・失敗・スキップ）か。
    pub fn is_finished(&self) -> bool {
        !self.items.is_empty()
            && self.items.iter().all(|item| {
                matches!(
                    item.status,
                    ItemStatus::Completed | ItemStatus::Failed | ItemStatus::Skipped
                )
            })
    }

    /// 開始・一時停止の見た目を切り替える。実際のダウンロードはまだ行わない。
    ///
    /// サイトごとに同時取得は1件までとし、サイトが異なれば同時に取得できる。
    /// 戻り値は、開始後に取得中の項目があるかどうか。
    pub fn set_running(&mut self, running: bool) -> bool {
        if running {
            let mut started = false;
            for site in Site::ALL {
                if self
                    .items
                    .iter()
                    .any(|item| item.site == site && item.status == ItemStatus::Downloading)
                {
                    started = true;
                    continue;
                }
                let next = self
                    .items
                    .iter()
                    .position(|item| item.site == site && item.status == ItemStatus::Paused)
                    .or_else(|| {
                        self.items
                            .iter()
                            .position(|item| item.site == site && item.status == ItemStatus::Queued)
                    });
                if let Some(index) = next {
                    self.items[index].status = ItemStatus::Downloading;
                    let label = Site::label(self.items[index].site);
                    let url = compact_url(&self.items[index].url);
                    self.push_log(
                        LogLevel::Info,
                        format!("[{label}] {url} の取得を開始しました（画面確認用の状態）"),
                    );
                    started = true;
                }
            }
            started
        } else {
            let mut paused = 0;
            for item in &mut self.items {
                if item.status == ItemStatus::Downloading {
                    item.status = ItemStatus::Paused;
                    paused += 1;
                }
            }
            if paused > 0 {
                self.push_log(LogLevel::Info, "キューを一時停止しました");
            }
            false
        }
    }

    /// 画質と音質を指定してURLを追加する。コンテナは共通設定の値を使う。
    ///
    /// `http://` または `https://` で始まる行だけを追加する。
    pub fn add_urls_with(
        &mut self,
        text: &str,
        quality: Quality,
        audio: AudioQuality,
        container: Container,
        destination: &str,
    ) -> usize {
        let urls: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.split_once("://").is_some_and(|(scheme, _)| {
                    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
                })
            })
            .collect();
        let container = container.for_quality(quality);
        let destination = if destination.trim().is_empty() {
            self.default_destination()
        } else {
            destination.to_string()
        };

        let mut added = 0;
        let mut added_sites = Vec::new();
        for url in urls {
            let id = self.next_id;
            self.next_id += 1;
            let item = QueueItem::new(id, url, quality, audio, container, destination.clone());
            added_sites.push(item.site);
            self.items.push(item);
            added += 1;
        }

        for site in Site::ALL {
            let count = added_sites.iter().filter(|added| **added == site).count();
            if count > 0 {
                self.push_log_for_site(
                    site,
                    LogLevel::Info,
                    format!(
                        "{count}件をキューに追加しました（画質 {} / 音質 {} / コンテナ {}）",
                        quality.short(),
                        audio.label(),
                        container.label()
                    ),
                );
            }
        }
        added
    }

    /// サイトの次に実行する項目を取得し、取得中に切り替える。
    pub fn take_next_for_site(&mut self, site: Site) -> Option<QueueItem> {
        let index = self.items.iter().position(|item| {
            item.site == site && matches!(item.status, ItemStatus::Paused | ItemStatus::Queued)
        })?;
        let item = &mut self.items[index];
        item.status = ItemStatus::Downloading;
        item.progress = 0.0;
        item.error = None;
        item.eta = None;
        item.downloaded = "—".into();
        item.total = "—".into();
        let url = compact_url(&item.url);
        let item = item.clone();
        self.push_log_for_site(site, LogLevel::Info, format!("{url} の取得を開始しました"));
        Some(item)
    }

    pub fn pause_site(&mut self, site: Site) {
        for item in self.items.iter_mut().filter(|item| item.site == site) {
            if item.status == ItemStatus::Downloading {
                item.status = ItemStatus::Paused;
                item.eta = None;
            }
        }
    }

    pub fn set_item_progress(
        &mut self,
        id: u64,
        progress: f32,
        downloaded: SharedString,
        total: SharedString,
        eta: Option<SharedString>,
    ) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.progress = progress.clamp(0.0, 100.0);
            item.downloaded = downloaded;
            item.total = total;
            item.eta = eta;
        }
    }

    pub fn complete_item(&mut self, id: u64, path: Option<&str>) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.status = ItemStatus::Completed;
            item.progress = 100.0;
            item.eta = None;
            if item.total == "—" {
                item.downloaded = "完了".into();
            } else {
                item.downloaded = item.total.clone();
            }
            item.error = None;
            item.output_path = path.map(Into::into);
            let site = item.site;
            let url = compact_url(&item.url);
            item.downloaded = item.downloaded.clone();
            let path = path.unwrap_or("（出力先はyt-dlpのログを確認してください）");
            self.push_log_for_site(
                site,
                LogLevel::Info,
                format!("[{url}] ダウンロード完了: {path}"),
            );
        }
    }

    /// yt-dlpの警告を、その項目のサイトのログへ記録する。
    pub fn warn_item(&mut self, id: u64, message: impl Into<SharedString>) {
        let Some(item) = self.items.iter().find(|item| item.id == id) else {
            return;
        };
        let site = item.site;
        let url = compact_url(&item.url);
        self.push_log_for_site(site, LogLevel::Warn, format!("[{url}] {}", message.into()));
    }

    pub fn fail_item(&mut self, id: u64, error: impl Into<SharedString>) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.status = ItemStatus::Failed;
            item.eta = None;
            item.attempts += 1;
            let error = error.into();
            item.error = Some(error.clone());
            let site = item.site;
            let url = compact_url(&item.url);
            self.push_log_for_site(site, LogLevel::Error, format!("[{url}] {error}"));
        }
    }

    pub fn pause_item(&mut self, id: u64) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id)
            && item.status == ItemStatus::Downloading
        {
            item.status = ItemStatus::Paused;
            item.eta = None;
        }
    }

    pub fn retry(&mut self, id: u64) {
        let url = if let Some(item) = self.items.iter_mut().find(|item| item.id == id)
            && item.status == ItemStatus::Failed
        {
            let site = item.site;
            item.status = ItemStatus::Queued;
            item.progress = 0.0;
            item.attempts = 0;
            item.error = None;
            item.output_path = None;
            item.downloaded = "—".into();
            item.total = "—".into();
            Some((site, compact_url(&item.url)))
        } else {
            None
        };
        if let Some((site, url)) = url {
            self.push_log_for_site(
                site,
                LogLevel::Info,
                format!("[{url}] を再試行キューに戻しました"),
            );
        }
    }

    pub fn retry_failed(&mut self) -> usize {
        let mut retried = 0;
        for item in &mut self.items {
            if item.status == ItemStatus::Failed {
                item.status = ItemStatus::Queued;
                item.progress = 0.0;
                item.attempts = 0;
                item.error = None;
                item.output_path = None;
                item.downloaded = "—".into();
                item.total = "—".into();
                retried += 1;
            }
        }
        if retried > 0 {
            self.push_log(
                LogLevel::Info,
                format!("失敗した{retried}件を再試行キューに戻しました"),
            );
        }
        retried
    }

    pub fn retry_failed_for_site(&mut self, site: Site) -> usize {
        let mut retried = 0;
        for item in self.items.iter_mut().filter(|item| item.site == site) {
            if item.status == ItemStatus::Failed {
                item.status = ItemStatus::Queued;
                item.progress = 0.0;
                item.attempts = 0;
                item.error = None;
                item.output_path = None;
                item.downloaded = "—".into();
                item.total = "—".into();
                retried += 1;
            }
        }
        if retried > 0 {
            self.push_log_for_site(
                site,
                LogLevel::Info,
                format!("失敗した{retried}件を再試行キューに戻しました"),
            );
        }
        retried
    }

    pub fn site_is_running(&self, site: Site) -> bool {
        self.items
            .iter()
            .any(|item| item.site == site && item.status == ItemStatus::Downloading)
    }

    #[cfg(test)]
    pub fn toggle_site_running(&mut self, site: Site) -> bool {
        if self.site_is_running(site) {
            for item in self.items.iter_mut().filter(|item| item.site == site) {
                if item.status == ItemStatus::Downloading {
                    item.status = ItemStatus::Paused;
                }
            }
            self.push_log_for_site(site, LogLevel::Info, "キューを一時停止しました");
            false
        } else {
            let next = self
                .items
                .iter()
                .position(|item| item.site == site && item.status == ItemStatus::Paused)
                .or_else(|| {
                    self.items
                        .iter()
                        .position(|item| item.site == site && item.status == ItemStatus::Queued)
                });
            if let Some(index) = next {
                self.items[index].status = ItemStatus::Downloading;
                let url = compact_url(&self.items[index].url);
                self.push_log_for_site(
                    site,
                    LogLevel::Info,
                    format!("{url} の取得を開始しました（画面確認用の状態）"),
                );
                true
            } else {
                false
            }
        }
    }

    pub fn clear_site(&mut self, site: Site) -> usize {
        let before = self.items.len();
        self.items.retain(|item| item.site != site);
        before - self.items.len()
    }

    /// 保存先に同じIDのダウンロード済みファイルがある項目を、取得せずスキップ済みにする。
    pub fn skip_already_downloaded(&mut self, id: u64, path: &str) {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        item.status = ItemStatus::Skipped;
        item.progress = 0.0;
        item.eta = None;
        item.error = None;
        item.output_path = Some(path.into());
        item.downloaded = "—".into();
        item.total = "—".into();
        let site = item.site;
        let url = compact_url(&item.url);
        self.push_log_for_site(
            site,
            LogLevel::Info,
            format!("[{url}] ダウンロード済みのファイルがあるためスキップしました: {path}"),
        );
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            let item = self.items.remove(index);
            self.push_log_for_site(
                item.site,
                LogLevel::Info,
                format!("[{}] をキューから削除しました", compact_url(&item.url)),
            );
        }
    }

    pub fn clear(&mut self) {
        let count = self.items.len();
        self.items.clear();
        if count > 0 {
            self.push_log(LogLevel::Info, format!("キューを空にしました（{count}件）"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_queue_starts_empty() {
        let model = QueueModel::new();

        assert!(model.items().is_empty());
        assert!(model.logs_for_site(Site::YouTube).is_empty());
        assert!(model.active_item().is_none());
        assert_eq!(model.destinations().len(), 1);
        assert_eq!(model.destinations()[0], default_downloads_dir());
    }

    #[test]
    fn completed_items_keep_their_output_path() {
        let mut model = QueueModel::new();
        model.add_urls_with(
            "https://example.com/video",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        let id = model.items().last().unwrap().id();

        model.complete_item(id, Some("/tmp/video.mp4"));

        let item = model.items().last().unwrap();
        assert_eq!(item.status(), ItemStatus::Completed);
        assert_eq!(item.output_path(), Some("/tmp/video.mp4"));

        model.retry(id);

        let item = model.items().last().unwrap();
        assert_eq!(item.status(), ItemStatus::Completed);
        assert_eq!(item.output_path(), Some("/tmp/video.mp4"));

        model.fail_item(id, "failed");
        model.retry(id);

        let item = model.items().last().unwrap();
        assert_eq!(item.status(), ItemStatus::Queued);
        assert_eq!(item.output_path(), None);

        model.complete_item(id, None);

        let item = model.items().last().unwrap();
        assert_eq!(item.status(), ItemStatus::Completed);
        assert_eq!(item.output_path(), None);
    }

    #[test]
    fn destinations_always_start_with_the_downloads_folder() {
        let mut model = QueueModel::new();
        model.set_destinations(vec!["/tmp/b".into(), "/tmp/a".into()]);

        assert_eq!(model.destinations().len(), 3);
        assert_eq!(model.destinations()[0], default_downloads_dir());
        assert_eq!(model.destinations()[1], "/tmp/b");
        assert_eq!(model.destinations()[2], "/tmp/a");
    }

    #[test]
    fn warnings_are_recorded_for_the_item_site() {
        let mut model = QueueModel::demo();
        let id = model
            .items()
            .iter()
            .find(|item| item.site() == Site::YouTube)
            .map(|item| item.id())
            .unwrap();

        model.warn_item(id, "WARNING: unable to fetch PO token");

        let logs = model.logs_for_site(Site::YouTube);
        assert!(
            logs.iter().any(
                |entry| entry.level() == LogLevel::Warn && entry.message().contains("PO token")
            )
        );
    }

    #[test]
    fn clear_logs_only_affects_the_selected_site() {
        let mut model = QueueModel::demo();
        assert!(!model.logs_for_site(Site::YouTube).is_empty());
        assert!(!model.logs_for_site(Site::X).is_empty());

        assert!(model.clear_logs_for_site(Site::YouTube) > 0);

        assert!(model.logs_for_site(Site::YouTube).is_empty());
        assert!(!model.logs_for_site(Site::X).is_empty());
    }

    #[test]
    fn add_urls_uses_default_quality_and_audio() {
        let mut model = QueueModel::demo();
        let before = model.items().len();

        let added = model.add_urls_with(
            " https://example.com/a \n\n https://example.com/b \n",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );

        assert_eq!(added, 2);
        assert_eq!(model.items().len(), before + 2);
        let added_item = model.items().last().unwrap();
        assert_eq!(added_item.url().to_string(), "https://example.com/b");
        assert_eq!(added_item.quality(), DEFAULT_QUALITY);
        assert_eq!(added_item.audio(), DEFAULT_AUDIO);
        assert_eq!(added_item.container(), DEFAULT_CONTAINER);
    }

    #[test]
    fn add_urls_ignores_lines_without_an_http_scheme() {
        let mut model = QueueModel::demo();
        let before = model.items().len();

        let added = model.add_urls_with(
            "example.com/a\nftp://example.com/b\n  http://example.com/c  \r\n\nhttps://example.com/d\nHTTPS://example.com/e\nhttpsx://example.com/f\n",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );

        assert_eq!(added, 3);
        assert_eq!(model.items().len(), before + 3);
        let added_urls: Vec<_> = model
            .items()
            .iter()
            .skip(before)
            .map(|item| item.url().to_string())
            .collect();
        assert_eq!(
            added_urls,
            vec![
                "http://example.com/c",
                "https://example.com/d",
                "HTTPS://example.com/e"
            ]
        );
    }

    #[test]
    fn add_urls_with_only_invalid_lines_adds_nothing() {
        let mut model = QueueModel::demo();
        let before = model.items().len();

        let added = model.add_urls_with(
            "example.com/a\nftp://example.com/b\njavascript:alert(1)\n",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );

        assert_eq!(added, 0);
        assert_eq!(model.items().len(), before);
    }

    #[test]
    fn add_urls_with_applies_selected_quality_audio_and_container() {
        let mut model = QueueModel::demo();
        let added = model.add_urls_with(
            "https://example.com/c",
            Quality::P480,
            AudioQuality::Mute,
            Container::Webm,
            &default_downloads_dir(),
        );

        assert_eq!(added, 1);
        let item = model.items().last().unwrap();
        assert_eq!(item.quality(), Quality::P480);
        assert_eq!(item.audio(), AudioQuality::Mute);
        assert_eq!(item.container(), Container::Webm);
    }

    #[test]
    fn audio_only_items_use_an_audio_container() {
        let mut model = QueueModel::demo();
        model.add_urls_with(
            "https://example.com/audio",
            Quality::AudioOnly,
            DEFAULT_AUDIO,
            Container::Mkv,
            &default_downloads_dir(),
        );

        let item = model.items().last().unwrap();
        assert_eq!(item.quality(), Quality::AudioOnly);
        assert_eq!(item.container(), DEFAULT_AUDIO_CONTAINER);
        assert!(
            Container::options_for(Quality::AudioOnly)
                .iter()
                .all(|container| container.is_audio())
        );
        assert!(
            Container::options_for(DEFAULT_QUALITY)
                .iter()
                .all(|container| !container.is_audio())
        );
    }

    #[test]
    fn bulk_setters_only_update_waiting_items() {
        let mut model = QueueModel::demo();
        model.add_urls_with(
            "https://www.youtube.com/watch?v=bulk",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        let before: Vec<_> = model
            .items()
            .iter()
            .filter(|item| item.site() == Site::YouTube && item.status() != ItemStatus::Queued)
            .map(|item| (item.id(), item.quality(), item.audio(), item.container()))
            .collect();
        assert!(!before.is_empty());

        model.set_all_quality_for_site(Site::YouTube, Quality::P720);
        model.set_all_audio_for_site(Site::YouTube, AudioQuality::Kbps192);
        model.set_all_container_for_site(Site::YouTube, Container::Webm);

        let waiting: Vec<_> = model
            .items()
            .iter()
            .filter(|item| item.site() == Site::YouTube && item.status() == ItemStatus::Queued)
            .collect();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].quality(), Quality::P720);
        assert_eq!(waiting[0].audio(), AudioQuality::Kbps192);
        assert_eq!(waiting[0].container(), Container::Webm);

        for (id, quality, audio, container) in before {
            let item = model.items().iter().find(|item| item.id() == id).unwrap();
            assert_eq!(item.quality(), quality);
            assert_eq!(item.audio(), audio);
            assert_eq!(item.container(), container);
        }
    }

    #[test]
    fn per_item_quality_and_audio_can_be_changed_while_waiting() {
        let mut model = QueueModel::new();
        model.add_urls_with(
            "https://example.com/a",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        let id = model.items().first().unwrap().id();

        model.set_item_quality(id, Quality::P720);
        model.set_item_audio(id, AudioQuality::Kbps128);

        let item = model.items().first().unwrap();
        assert_eq!(item.quality(), Quality::P720);
        assert_eq!(item.audio(), AudioQuality::Kbps128);
    }

    #[test]
    fn started_items_keep_their_settings() {
        let mut model = QueueModel::new();
        model.add_urls_with(
            "https://example.com/a",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        let id = model.items().first().unwrap().id();
        model.items[0].status = ItemStatus::Downloading;

        model.set_item_quality(id, Quality::P480);
        model.set_item_audio(id, AudioQuality::Mute);
        model.set_item_container(id, Container::Webm);
        model.set_item_destination(id, "/tmp/other");

        let item = &model.items[0];
        assert_eq!(item.quality(), DEFAULT_QUALITY);
        assert_eq!(item.audio(), DEFAULT_AUDIO);
        assert_eq!(item.container(), DEFAULT_CONTAINER);
        assert_eq!(item.destination(), &default_downloads_dir());
    }

    #[test]
    fn changing_quality_keeps_the_container_kind_in_sync() {
        let mut model = QueueModel::new();
        let destination = default_downloads_dir();
        model.add_urls_with(
            "https://www.youtube.com/watch?v=a\nhttps://www.youtube.com/watch?v=b",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &destination,
        );
        let id = model.items().first().unwrap().id();

        model.set_item_quality(id, Quality::AudioOnly);
        assert_eq!(model.items().first().unwrap().container(), Container::M4a);

        model.set_item_quality(id, Quality::P720);
        assert_eq!(model.items().first().unwrap().container(), Container::Mkv);

        model.set_all_quality_for_site(Site::YouTube, Quality::AudioOnly);
        assert!(
            model
                .items()
                .iter()
                .filter(|item| item.site() == Site::YouTube)
                .all(|item| item.container().is_audio())
        );
    }

    #[test]
    fn retry_failed_returns_items_to_the_queue() {
        let mut model = QueueModel::demo();
        assert!(model.failed_count() > 0);

        assert!(model.retry_failed() > 0);

        assert_eq!(model.failed_count(), 0);
        assert!(
            model
                .items()
                .iter()
                .any(|item| item.status() == ItemStatus::Queued)
        );
    }

    #[test]
    fn site_is_detected_from_url_host() {
        assert_eq!(
            Site::from_url("https://www.youtube.com/watch?v=x"),
            Site::YouTube
        );
        assert_eq!(Site::from_url("https://youtu.be/x"), Site::YouTube);
        assert_eq!(Site::from_url("https://x.com/user/status/1"), Site::X);
        assert_eq!(Site::from_url("https://twitter.com/user/status/1"), Site::X);
        assert_eq!(Site::from_url("https://www.twitch.tv/user"), Site::Twitch);
        assert_eq!(Site::from_url("https://vimeo.com/1"), Site::Other);
        assert_eq!(Site::from_url("https://notyoutube.com/x"), Site::Other);
    }

    #[test]
    fn urls_are_grouped_by_site_and_destination() {
        let mut model = QueueModel::demo();
        let destination = "/tmp/downloads";
        model.add_urls_with(
            "https://www.youtube.com/watch?v=test1\nhttps://x.com/test/status/1",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            destination,
        );

        let added: Vec<_> = model
            .items()
            .iter()
            .filter(|item| item.destination() == destination)
            .collect();
        assert_eq!(added.len(), 2);
        assert!(added.iter().any(|item| item.site() == Site::YouTube));
        assert!(added.iter().any(|item| item.site() == Site::X));
    }

    #[test]
    fn running_keeps_at_most_one_active_item_per_site() {
        let mut model = QueueModel::demo();
        model.set_running(false);
        model.set_running(true);

        for site in Site::ALL {
            let active = model
                .items()
                .iter()
                .filter(|item| item.site() == site && item.status() == ItemStatus::Downloading)
                .count();
            assert!(active <= 1, "{site:?} has {active} active items");
        }
    }

    #[test]
    fn site_actions_are_scoped() {
        let mut model = QueueModel::demo();
        let twitch_count = model
            .items()
            .iter()
            .filter(|item| item.site() == Site::Twitch)
            .count();
        assert!(twitch_count > 0);
        model.add_urls_with(
            "https://x.com/test/status/2",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );

        assert!(model.toggle_site_running(Site::X));
        assert!(model.site_is_running(Site::X));
        assert!(!model.toggle_site_running(Site::X));
        assert!(!model.site_is_running(Site::X));

        let retried = model.retry_failed_for_site(Site::Other);
        assert!(retried > 0);
        assert!(
            !model
                .items()
                .iter()
                .any(|item| item.site() == Site::Other && item.status() == ItemStatus::Failed)
        );

        model.clear_site(Site::Twitch);
        assert!(!model.items().iter().any(|item| item.site() == Site::Twitch));
        assert!(!model.logs_for_site(Site::YouTube).is_empty());
    }

    #[test]
    fn pending_items_are_reported() {
        let mut model = QueueModel::new();
        assert!(!model.has_pending());

        model.add_urls_with(
            "https://example.com/a",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );

        assert!(model.has_pending());
    }

    #[test]
    fn queue_is_finished_only_when_every_item_has_ended() {
        let mut model = QueueModel::new();
        assert!(!model.is_finished());

        model.add_urls_with(
            "https://example.com/a\nhttps://example.com/b",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        assert!(!model.is_finished());

        model.items[0].status = ItemStatus::Completed;
        assert!(!model.is_finished());

        model.items[1].status = ItemStatus::Failed;
        assert!(model.is_finished());

        model.items[1].status = ItemStatus::Paused;
        assert!(!model.is_finished());

        model.items[1].status = ItemStatus::Skipped;
        assert!(model.is_finished());
    }

    #[test]
    fn skipping_an_already_downloaded_item_keeps_the_reason_in_the_log() {
        let mut model = QueueModel::demo();
        let id = model.items().first().unwrap().id();

        model.skip_already_downloaded(id, "/tmp/downloads/example [abc123].mp4");

        let item = model.items().first().unwrap();
        assert_eq!(item.status(), ItemStatus::Skipped);
        assert_eq!(
            item.output_path(),
            Some("/tmp/downloads/example [abc123].mp4")
        );
        assert!(model.logs_for_site(Site::YouTube).iter().any(|entry| {
            entry
                .message()
                .contains("ダウンロード済みのファイルがあるため")
        }));
    }

    #[test]
    fn pausing_stops_the_active_item() {
        let mut model = QueueModel::demo();
        model.add_urls_with(
            "https://www.youtube.com/watch?v=pause-test",
            DEFAULT_QUALITY,
            DEFAULT_AUDIO,
            DEFAULT_CONTAINER,
            &default_downloads_dir(),
        );
        assert!(model.set_running(true));
        assert!(model.active_item().is_some());

        model.set_running(false);

        assert!(model.active_item().is_none());
        assert!(
            model
                .items()
                .iter()
                .any(|item| item.status() == ItemStatus::Paused)
        );
    }
}
