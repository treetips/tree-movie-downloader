//! アプリ全体の設定と、同梱・ユーザー提供ファイルの探索。

use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui_kit::Hsla;
use serde::{Deserialize, Serialize};
use sys_locale::get_locale;

pub const APP_DIR_NAME: &str = "tree-movie-downloader";
pub const APP_NAME: &str = "TreeMovieDownloader";

/// ダウンロードファイル名の初期テンプレート。
pub const DEFAULT_FILENAME_TEMPLATE: &str = "${uploadDate}_${title} [${duration}][${width}x${height} ${vcodec} ${fps} ${acodec} ${abr}][${id}].${ext}";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceMode {
    #[default]
    Auto,
    Light,
    Dark,
}

impl AppearanceMode {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自動",
            Self::Light => "ライト",
            Self::Dark => "ダーク",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Auto)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "ja-JP")]
    Japanese,
    #[serde(rename = "en-US")]
    English,
}

impl Default for Language {
    fn default() -> Self {
        locale_language()
    }
}

impl Language {
    pub const ALL: [Self; 2] = [Self::Japanese, Self::English];

    pub fn label(self) -> &'static str {
        match self {
            Self::Japanese => "日本語 (ja-JP)",
            Self::English => "英語 (en-US)",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(1)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::English)
    }
}

fn locale_language() -> Language {
    match get_locale() {
        Some(locale) if locale.to_lowercase().starts_with("ja") => Language::Japanese,
        _ => Language::English,
    }
}

/// YouTubeの認証に使う、ログイン済みブラウザのCookie取得元。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CookieBrowser {
    /// Cookieを使わない。
    #[default]
    None,
    Safari,
    Chrome,
    Firefox,
    Edge,
    Brave,
    Vivaldi,
    Chromium,
    Opera,
    Whale,
}

impl CookieBrowser {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Safari,
        Self::Chrome,
        Self::Firefox,
        Self::Edge,
        Self::Brave,
        Self::Vivaldi,
        Self::Chromium,
        Self::Opera,
        Self::Whale,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "使用しない",
            Self::Safari => "Safari",
            Self::Chrome => "Chrome",
            Self::Firefox => "Firefox",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            Self::Vivaldi => "Vivaldi",
            Self::Chromium => "Chromium",
            Self::Opera => "Opera",
            Self::Whale => "Whale",
        }
    }

    /// `yt-dlp --cookies-from-browser` に渡す値。
    pub fn yt_dlp_value(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Safari => Some("safari"),
            Self::Chrome => Some("chrome"),
            Self::Firefox => Some("firefox"),
            Self::Edge => Some("edge"),
            Self::Brave => Some("brave"),
            Self::Vivaldi => Some("vivaldi"),
            Self::Chromium => Some("chromium"),
            Self::Opera => Some("opera"),
            Self::Whale => Some("whale"),
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|browser| *browser == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::None)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontScale {
    Small,
    #[default]
    Medium,
    Large,
}

impl FontScale {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "小さい",
            Self::Medium => "標準",
            Self::Large => "大きい",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|scale| *scale == self)
            .unwrap_or(1)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Medium)
    }

    pub fn px(self) -> f32 {
        match self {
            Self::Small => 14.0,
            Self::Medium => 16.0,
            Self::Large => 18.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShuffleInterval {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    #[default]
    OneHour,
    FourHours,
    TwelveHours,
    OneDay,
}

impl ShuffleInterval {
    pub const ALL: [Self; 8] = [
        Self::OneMinute,
        Self::FiveMinutes,
        Self::FifteenMinutes,
        Self::ThirtyMinutes,
        Self::OneHour,
        Self::FourHours,
        Self::TwelveHours,
        Self::OneDay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1分ごと",
            Self::FiveMinutes => "5分ごと",
            Self::FifteenMinutes => "15分ごと",
            Self::ThirtyMinutes => "30分ごと",
            Self::OneHour => "1時間ごと",
            Self::FourHours => "4時間ごと",
            Self::TwelveHours => "12時間ごと",
            Self::OneDay => "1日ごと",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|i| *i == self).unwrap_or(4)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::OneHour)
    }

    pub fn duration(self) -> std::time::Duration {
        use std::time::Duration;

        match self {
            Self::OneMinute => Duration::from_secs(60),
            Self::FiveMinutes => Duration::from_secs(5 * 60),
            Self::FifteenMinutes => Duration::from_secs(15 * 60),
            Self::ThirtyMinutes => Duration::from_secs(30 * 60),
            Self::OneHour => Duration::from_secs(60 * 60),
            Self::FourHours => Duration::from_secs(4 * 60 * 60),
            Self::TwelveHours => Duration::from_secs(12 * 60 * 60),
            Self::OneDay => Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// ユーザーが編集する設定・サウンド・背景画像のディレクトリ。
/// `~/.config/tree-movie-downloader`。
pub fn config_dir() -> PathBuf {
    home_dir().join(".config").join(APP_DIR_NAME)
}

/// アプリが管理する外部ツールのディレクトリ。
///
/// macOSでは `~/Library/Application Support/tree-movie-downloader/tools`。
/// それ以外のOSでは既存の設定ディレクトリ配下を使う。
pub fn tools_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir()
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME)
            .join("tools")
    }
    #[cfg(not(target_os = "macos"))]
    {
        config_dir().join("tools")
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// 表示用のアプリバージョン。
///
/// リリースビルドでは、タグから算出したバージョンをビルド時に注入する。
/// 開発ビルドでは `Cargo.toml` のバージョンを使う。
pub const APP_VERSION: &str = match option_env!("TMD_APP_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// 同梱アセットのディレクトリ。
///
/// `.app` として起動した場合は `Contents/Resources/assets` を、
/// `cargo run` で起動した場合はリポジトリ直下の `assets` を使う。
pub fn assets_dir() -> PathBuf {
    bundle_resources_dir()
        .map(|resources| resources.join("assets"))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))
}

fn bundle_resources_dir() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    bundle_resources_for(&executable)
}

fn bundle_resources_for(executable: &Path) -> Option<PathBuf> {
    let macos_dir = executable.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }
    Some(contents_dir.join("Resources"))
}

/// 設定ファイル。存在しない、または壊れている場合は既定値を使う。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub notify_on_complete: bool,
    pub play_sound: bool,
    pub success_sound: Option<String>,
    pub success_random: bool,
    pub error_sound: Option<String>,
    pub error_random: bool,
    pub appearance: AppearanceMode,
    pub language: Language,
    pub font_scale: FontScale,
    pub wallpaper: Option<String>,
    pub wallpaper_random: bool,
    pub wallpaper_interval: ShuffleInterval,
    pub wallpaper_opacity: f32,
    pub background_color: Option<String>,
    pub filename_template: String,
    pub destinations: Vec<String>,
    pub container_index: usize,
    pub retries: usize,
    pub cookies_from_browser: CookieBrowser,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            notify_on_complete: true,
            play_sound: false,
            success_sound: None,
            success_random: false,
            error_sound: None,
            error_random: false,
            appearance: AppearanceMode::Auto,
            language: Language::default(),
            font_scale: FontScale::default(),
            wallpaper: None,
            wallpaper_random: false,
            wallpaper_interval: ShuffleInterval::default(),
            wallpaper_opacity: 1.0,
            background_color: None,
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_string(),
            destinations: Vec::new(),
            container_index: 1,
            retries: 3,
            cookies_from_browser: CookieBrowser::None,
        }
    }
}

impl AppSettings {
    pub fn load() -> Self {
        Self::load_from(&settings_path())
    }

    fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&settings_path())
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }
}

#[derive(Clone, Debug)]
pub struct SoundOption {
    pub label: String,
    pub id: String,
    pub path: PathBuf,
}

const SOUND_EXTENSIONS: [&str; 8] = ["mp3", "wav", "flac", "m4a", "aac", "ogg", "aiff", "caf"];

/// 同梱サウンドとユーザーサウンドを、指定した種別について列挙する。
pub fn scan_sounds(kind: &str) -> Vec<SoundOption> {
    let mut options = Vec::new();

    let bundled = assets_dir().join("sounds").join(kind);
    for path in files_with_extensions(&bundled, &SOUND_EXTENSIONS) {
        let name = file_name(&path);
        options.push(SoundOption {
            label: format!("（サンプル）{name}"),
            id: format!("bundled:{kind}/{name}"),
            path,
        });
    }

    let user = config_dir().join("sounds").join(kind);
    for path in files_with_extensions(&user, &SOUND_EXTENSIONS) {
        let name = file_name(&path);
        options.push(SoundOption {
            label: name.clone(),
            id: format!("user:{}", path.display()),
            path,
        });
    }

    options
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WallpaperSource {
    Bundled(String),
    User(PathBuf),
}

#[derive(Clone, Debug)]
pub struct WallpaperOption {
    pub label: String,
    pub id: String,
    pub source: WallpaperSource,
}

const WALLPAPER_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "jxl", "webp"];

/// 背景画像の選択肢。同梱画像を先に、その後にユーザー画像を並べる。
pub fn scan_wallpapers() -> Vec<WallpaperOption> {
    let mut options = Vec::new();

    let bundled = assets_dir().join("images").join("wallpaper");
    for path in files_with_extensions(&bundled, &WALLPAPER_EXTENSIONS) {
        let name = file_name(&path);
        options.push(WallpaperOption {
            label: format!("（サンプル）{name}"),
            id: format!("bundled:{name}"),
            source: WallpaperSource::Bundled(name),
        });
    }

    let user = config_dir().join("wallpaper");
    for path in files_with_extensions(&user, &WALLPAPER_EXTENSIONS) {
        if fs::File::open(&path).is_err() {
            continue;
        }
        let name = file_name(&path);
        options.push(WallpaperOption {
            label: name.clone(),
            id: format!("user:{}", path.display()),
            source: WallpaperSource::User(path),
        });
    }

    options
}

fn files_with_extensions(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        extensions
                            .iter()
                            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                    })
                    .unwrap_or(false)
        })
        .collect();
    paths.sort_by_key(|path| file_name(path).to_lowercase());
    paths
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub fn play_sound(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/afplay")
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub fn parse_hex(text: &str) -> Option<Hsla> {
    gpui_kit::Rgba::try_from(text.trim()).ok().map(Hsla::from)
}

pub fn hex_string(color: Hsla) -> String {
    let rgba = color.to_rgb();
    format!(
        "#{:02X}{:02X}{:02X}",
        (rgba.r * 255.0).round().clamp(0.0, 255.0) as u8,
        (rgba.g * 255.0).round().clamp(0.0, 255.0) as u8,
        (rgba.b * 255.0).round().clamp(0.0, 255.0) as u8
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_round_trip() {
        for hex in ["#000000", "#FFFFFF", "#1E1E1E", "#F0F4F8"] {
            let color = parse_hex(hex).expect("valid hex");
            assert_eq!(hex_string(color), hex);
        }
        assert!(parse_hex("not-a-color").is_none());
    }

    #[test]
    fn settings_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("tmd-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = AppSettings {
            appearance: AppearanceMode::Light,
            font_scale: FontScale::Small,
            play_sound: true,
            wallpaper_opacity: 0.25,
            destinations: vec!["/tmp/saved".into()],
            cookies_from_browser: CookieBrowser::Chrome,
            ..AppSettings::default()
        };

        settings.save_to(&path).expect("save settings");
        let loaded = AppSettings::load_from(&path);

        assert_eq!(loaded.appearance, AppearanceMode::Light);
        assert_eq!(loaded.font_scale, FontScale::Small);
        assert!(loaded.play_sound);
        assert_eq!(loaded.wallpaper_opacity, 0.25);
        assert_eq!(loaded.destinations, vec!["/tmp/saved"]);
        assert_eq!(loaded.cookies_from_browser, CookieBrowser::Chrome);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cookie_browser_maps_to_yt_dlp_values() {
        assert_eq!(
            AppSettings::default().cookies_from_browser,
            CookieBrowser::None
        );
        assert_eq!(CookieBrowser::None.yt_dlp_value(), None);
        assert_eq!(CookieBrowser::Chrome.yt_dlp_value(), Some("chrome"));
        assert_eq!(CookieBrowser::Safari.yt_dlp_value(), Some("safari"));
        for browser in CookieBrowser::ALL {
            assert_eq!(CookieBrowser::from_index(browser.index()), browser);
        }
    }

    #[test]
    fn shuffle_intervals_have_expected_durations() {
        use std::time::Duration;

        assert_eq!(
            ShuffleInterval::OneMinute.duration(),
            Duration::from_secs(60)
        );
        assert_eq!(
            ShuffleInterval::FiveMinutes.duration(),
            Duration::from_secs(5 * 60)
        );
        assert_eq!(
            ShuffleInterval::OneHour.duration(),
            Duration::from_secs(60 * 60)
        );
        assert_eq!(
            ShuffleInterval::OneDay.duration(),
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = AppSettings {
            appearance: AppearanceMode::Dark,
            font_scale: FontScale::Large,
            play_sound: true,
            success_random: true,
            wallpaper: Some("bundled:wallpaper1.jpg".into()),
            wallpaper_opacity: 0.5,
            background_color: Some("#1E1E1E".into()),
            filename_template: "custom-${title}.${ext}".into(),
            destinations: vec!["/tmp/a".into(), "/tmp/b".into()],
            ..AppSettings::default()
        };

        let json = serde_json::to_string(&settings).expect("serialize settings");
        let restored: AppSettings = serde_json::from_str(&json).expect("deserialize settings");

        assert_eq!(restored.appearance, AppearanceMode::Dark);
        assert_eq!(restored.font_scale, FontScale::Large);
        assert!(restored.play_sound);
        assert!(restored.success_random);
        assert_eq!(
            restored.wallpaper.as_deref(),
            Some("bundled:wallpaper1.jpg")
        );
        assert_eq!(restored.wallpaper_opacity, 0.5);
        assert_eq!(restored.background_color.as_deref(), Some("#1E1E1E"));
        assert_eq!(restored.filename_template, "custom-${title}.${ext}");
        assert_eq!(restored.destinations, vec!["/tmp/a", "/tmp/b"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_tools_use_application_support_on_macos() {
        let expected = home_dir()
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME)
            .join("tools");

        assert_eq!(tools_dir(), expected);
        assert_eq!(config_dir(), home_dir().join(".config").join(APP_DIR_NAME));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn managed_tools_stay_in_the_config_directory_on_other_platforms() {
        assert_eq!(tools_dir(), config_dir().join("tools"));
    }

    #[test]
    fn finds_resources_inside_a_macos_app_bundle() {
        let root = std::env::temp_dir().join(format!(
            "tmd-bundle-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let macos_dir = root.join("Contents").join("MacOS");
        let assets_dir = root.join("Contents").join("Resources").join("assets");
        std::fs::create_dir_all(&macos_dir).unwrap();
        std::fs::create_dir_all(&assets_dir).unwrap();

        let resources = bundle_resources_for(&macos_dir.join("tree-movie-downloader"));
        assert_eq!(resources, Some(root.join("Contents").join("Resources")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_non_bundle_executables() {
        assert!(bundle_resources_for(Path::new("/tmp/tree-movie-downloader")).is_none());
    }

    #[test]
    fn exposes_a_non_empty_app_version() {
        assert!(!APP_VERSION.is_empty());
    }

    #[test]
    fn app_name_matches_release_workflow() {
        let workflow_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
        let Ok(workflow) = std::fs::read_to_string(&workflow_path) else {
            eprintln!(
                "リリースワークフローが見つからないため確認をスキップします: {}",
                workflow_path.display()
            );
            return;
        };
        let declared = workflow
            .lines()
            .find_map(|line| line.trim().strip_prefix("APP_NAME:"))
            .map(|value| {
                value
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches(|character| character == '\'' || character == '"')
            });
        assert_eq!(
            declared,
            Some(APP_NAME),
            "リリースワークフローの APP_NAME が Rust 定数と一致しません"
        );
    }
}
