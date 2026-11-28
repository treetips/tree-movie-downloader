//! 設定ページ。アプリ全体の設定と、キュー全体に共通するダウンロード設定を扱う。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IndexPath, Sizable as _, StyledExt as _, Theme, ThemeMode,
    WindowExt as _,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    hover_card::HoverCard,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    notification::Notification,
    radio::{Radio, RadioGroup},
    select::{Select, SelectEvent, SelectState},
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_settings::{
    AppSettings, AppearanceMode, CookieBrowser, FontScale, Language, ShuffleInterval, SoundOption,
    WallpaperOption, WallpaperSource, hex_string, parse_hex, play_sound, scan_sounds,
    scan_wallpapers,
};

/// ファイル名テンプレートで使える変数。
const FILENAME_VARIABLES: [(&str, &str); 11] = [
    ("uploadDate", "アップロード日"),
    ("title", "動画タイトル"),
    ("duration", "再生時間"),
    ("width", "動画の幅"),
    ("height", "動画の高さ"),
    ("vcodec", "映像コーデック"),
    ("fps", "フレームレート"),
    ("acodec", "音声コーデック"),
    ("abr", "音声ビットレート"),
    ("id", "動画ID"),
    ("ext", "ファイル拡張子"),
];
use crate::model::{
    AudioQuality, Container, DEFAULT_AUDIO_CONTAINER, DEFAULT_QUALITY, DownloadSettings, Quality,
    QueueEvent, QueueModel, RETRY_CHOICES, RETRY_LABELS, default_downloads_dir,
};
use crate::wallpaper::{WallpaperState, wallpaper_image};

const DOWNLOAD_QUALITY_CHOICES: [Quality; 4] = [
    Quality::P1080,
    Quality::P720,
    Quality::P480,
    Quality::AudioOnly,
];
const MP4_STRENGTH: &str = "汎用性が高く、多くの機器・アプリで再生しやすい形式です。";
const MKV_STRENGTH: &str = "複数の映像・音声・字幕トラックを1つのファイルにまとめられるため、柔軟な収録・編集に向いています。";
const WEBM_STRENGTH: &str = "軽量で、ブラウザでの再生やWeb配信に適しています。";

/// 設定ページの状態。
pub struct SettingsPage {
    queue: Entity<QueueModel>,
    wallpaper_state: Entity<WallpaperState>,
    settings: AppSettings,

    success_sounds: Vec<SoundOption>,
    error_sounds: Vec<SoundOption>,
    wallpapers: Vec<WallpaperOption>,

    success_select: Entity<SelectState<Vec<SharedString>>>,
    error_select: Entity<SelectState<Vec<SharedString>>>,
    language_select: Entity<SelectState<Vec<SharedString>>>,
    wallpaper_select: Entity<SelectState<Vec<SharedString>>>,
    interval_select: Entity<SelectState<Vec<SharedString>>>,
    cookie_select: Entity<SelectState<Vec<SharedString>>>,

    opacity_slider: Entity<SliderState>,
    color_picker: Entity<ColorPickerState>,
    hex_input: Entity<InputState>,
    filename_input: Entity<InputState>,
    destination_input: Entity<TextareaState>,
    hex_invalid: bool,
    shuffle_task: Option<Task<()>>,

    _subscriptions: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(
        queue: Entity<QueueModel>,
        wallpaper_state: Entity<WallpaperState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = AppSettings::load();
        if !DOWNLOAD_QUALITY_CHOICES
            .iter()
            .any(|choice| choice.index() == settings.quality_index)
        {
            settings.quality_index = DEFAULT_QUALITY.index();
        }
        apply_theme(&settings, window, cx);

        let success_sounds = scan_sounds("success");
        let error_sounds = scan_sounds("error");
        let wallpapers = scan_wallpapers();

        if let Some(id) = settings.wallpaper.as_deref()
            && !wallpapers.iter().any(|wallpaper| wallpaper.id == id)
        {
            settings.wallpaper = wallpapers
                .iter()
                .find(|wallpaper| matches!(wallpaper.source, WallpaperSource::Bundled(_)))
                .map(|wallpaper| wallpaper.id.clone());
        }

        let success_labels: Vec<SharedString> = success_sounds
            .iter()
            .map(|sound| SharedString::from(sound.label.clone()))
            .collect();
        let error_labels: Vec<SharedString> = error_sounds
            .iter()
            .map(|sound| SharedString::from(sound.label.clone()))
            .collect();
        let success_index =
            selected_sound_index(settings.success_sound.as_deref(), &success_sounds);
        let error_index = selected_sound_index(settings.error_sound.as_deref(), &error_sounds);

        let language_labels: Vec<SharedString> = Language::ALL
            .iter()
            .map(|language| SharedString::from(language.label()))
            .collect();
        let mut wallpaper_labels = vec![SharedString::from("背景無し")];
        wallpaper_labels.extend(
            wallpapers
                .iter()
                .map(|wallpaper| SharedString::from(wallpaper.label.clone())),
        );
        let wallpaper_index = settings
            .wallpaper
            .as_deref()
            .and_then(|id| wallpapers.iter().position(|wallpaper| wallpaper.id == id))
            .map(|index| index + 1)
            .unwrap_or(0);
        let interval_labels: Vec<SharedString> = ShuffleInterval::ALL
            .iter()
            .map(|interval| SharedString::from(interval.label()))
            .collect();
        let cookie_labels: Vec<SharedString> = CookieBrowser::ALL
            .iter()
            .map(|browser| SharedString::from(browser.label()))
            .collect();

        let success_select = cx.new(|cx| {
            SelectState::new(
                success_labels,
                success_index.map(IndexPath::new),
                window,
                cx,
            )
        });
        let error_select = cx
            .new(|cx| SelectState::new(error_labels, error_index.map(IndexPath::new), window, cx));
        let language_select = cx.new(|cx| {
            SelectState::new(
                language_labels,
                Some(IndexPath::new(settings.language.index())),
                window,
                cx,
            )
        });
        let wallpaper_select = cx.new(|cx| {
            SelectState::new(
                wallpaper_labels,
                Some(IndexPath::new(wallpaper_index)),
                window,
                cx,
            )
        });
        let initial_wallpaper = settings
            .wallpaper
            .as_deref()
            .and_then(|id| wallpapers.iter().find(|wallpaper| wallpaper.id == id))
            .map(|wallpaper| wallpaper.source.clone());
        wallpaper_state.update(cx, |state, _| {
            state.source = initial_wallpaper;
            state.opacity = settings.wallpaper_opacity.clamp(0.0, 1.0);
        });

        let interval_select = cx.new(|cx| {
            SelectState::new(
                interval_labels,
                Some(IndexPath::new(settings.wallpaper_interval.index())),
                window,
                cx,
            )
        });
        let cookie_select = cx.new(|cx| {
            SelectState::new(
                cookie_labels,
                Some(IndexPath::new(settings.cookies_from_browser.index())),
                window,
                cx,
            )
        });

        let opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(settings.wallpaper_opacity.clamp(0.0, 1.0))
        });

        let initial_color = settings
            .background_color
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(cx.theme().background);
        let color_picker =
            cx.new(|cx| ColorPickerState::new(window, cx).default_value(initial_color));
        let hex_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(hex_string(initial_color))
                .placeholder("#FFFFFF")
        });
        let filename_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.filename_template.clone())
                .placeholder("${title}.${ext}")
        });

        let initial_destinations = parse_destinations(&settings.destinations.join("\n"));
        settings.destinations = initial_destinations.clone();
        let destination_text = initial_destinations.join("\n");
        let destination_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .default_value(destination_text)
                .placeholder("/Users/yourname/Movies\n/Volumes/Archive/Movies")
        });
        let destination_subscription = cx.subscribe_in(
            &destination_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    this.persist_destinations(window, cx);
                }
            },
        );
        queue.update(cx, |model, _| {
            model.set_destinations(initial_destinations);
            model.set_quality(Quality::from_index(settings.quality_index));
            model.set_audio(AudioQuality::from_index(settings.audio_index));
            model.set_container(Container::from_index(settings.container_index));
            model.set_retries(settings.retries);
        });

        let success_subscription = cx.subscribe_in(
            &success_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                let Some(sound) = this.success_sounds.get(index.row) else {
                    return;
                };
                this.settings.success_sound = Some(sound.id.clone());
                this.save(window, cx);
                cx.notify();
            },
        );
        let error_subscription = cx.subscribe_in(
            &error_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                let Some(sound) = this.error_sounds.get(index.row) else {
                    return;
                };
                this.settings.error_sound = Some(sound.id.clone());
                this.save(window, cx);
                cx.notify();
            },
        );
        let language_subscription = cx.subscribe_in(
            &language_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                this.settings.language = Language::from_index(index.row);
                this.save(window, cx);
                cx.notify();
            },
        );
        let wallpaper_subscription = cx.subscribe_in(
            &wallpaper_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                this.settings.wallpaper = if index.row == 0 {
                    None
                } else {
                    this.wallpapers
                        .get(index.row - 1)
                        .map(|wallpaper| wallpaper.id.clone())
                };
                let source = this
                    .settings
                    .wallpaper
                    .as_deref()
                    .and_then(|id| this.wallpapers.iter().find(|wallpaper| wallpaper.id == id))
                    .map(|wallpaper| wallpaper.source.clone());
                this.wallpaper_state.update(cx, |state, cx| {
                    state.source = source;
                    cx.notify();
                });
                this.save(window, cx);
                cx.notify();
            },
        );
        let interval_subscription = cx.subscribe_in(
            &interval_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                this.settings.wallpaper_interval = ShuffleInterval::from_index(index.row);
                this.save(window, cx);
                this.restart_wallpaper_shuffle(window, cx);
                cx.notify();
            },
        );
        let cookie_subscription = cx.subscribe_in(
            &cookie_select,
            window,
            |this, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if !matches!(event, SelectEvent::Confirm(_)) {
                    return;
                }
                let Some(index) = state.read(cx).selected_index(cx) else {
                    return;
                };
                this.settings.cookies_from_browser = CookieBrowser::from_index(index.row);
                this.save(window, cx);
                cx.notify();
            },
        );
        let slider_subscription = cx.subscribe_in(
            &opacity_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                let (value, released) = match event {
                    SliderEvent::Change(value) => (value.end(), false),
                    SliderEvent::Release(value) => (value.end(), true),
                };
                this.settings.wallpaper_opacity = value.clamp(0.0, 1.0);
                this.wallpaper_state.update(cx, |state, cx| {
                    state.opacity = value.clamp(0.0, 1.0);
                    cx.notify();
                });
                if released {
                    this.save(window, cx);
                }
                cx.notify();
            },
        );
        let color_subscription = cx.subscribe_in(
            &color_picker,
            window,
            |this, _, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(value) = event;
                if let Some(color) = value {
                    let hex = hex_string(*color);
                    this.settings.background_color = Some(hex.clone());
                    this.hex_invalid = false;
                    this.hex_input
                        .update(cx, |input, cx| input.set_value(hex, window, cx));
                    Theme::update(cx, |theme| theme.colors.background = *color);
                    this.save(window, cx);
                    cx.notify();
                }
            },
        );
        let hex_subscription = cx.subscribe_in(
            &hex_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if this.hex_invalid {
                        this.hex_invalid = false;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => {
                    let text = this.hex_input.read(cx).value();
                    if let Some(color) = parse_hex(&text) {
                        let hex = hex_string(color);
                        this.hex_invalid = false;
                        this.settings.background_color = Some(hex.clone());
                        this.color_picker
                            .update(cx, |picker, cx| picker.set_value(color, window, cx));
                        this.hex_input
                            .update(cx, |input, cx| input.set_value(hex, window, cx));
                        Theme::update(cx, |theme| theme.colors.background = color);
                        this.save(window, cx);
                        cx.notify();
                    } else {
                        this.hex_invalid = true;
                        let fallback = this.current_background_hex(cx);
                        this.hex_input
                            .update(cx, |input, cx| input.set_value(fallback, window, cx));
                        cx.notify();
                    }
                }
                _ => {}
            },
        );

        let filename_subscription = cx.subscribe_in(
            &filename_input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    this.settings.filename_template = input.read(cx).value().to_string();
                    this.save(window, cx);
                    cx.notify();
                }
            },
        );

        Self {
            queue,
            wallpaper_state,
            settings,
            success_sounds,
            error_sounds,
            wallpapers,
            success_select,
            error_select,
            language_select,
            wallpaper_select,
            interval_select,
            cookie_select,
            opacity_slider,
            color_picker,
            hex_input,
            filename_input,
            destination_input,
            hex_invalid: false,
            shuffle_task: None,
            _subscriptions: vec![
                success_subscription,
                error_subscription,
                language_subscription,
                wallpaper_subscription,
                interval_subscription,
                cookie_subscription,
                slider_subscription,
                color_subscription,
                hex_subscription,
                filename_subscription,
                destination_subscription,
            ],
        }
    }

    /// システムの外観が変わったとき、自動モードならテーマを追従させる。
    pub fn apply_system_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.appearance != AppearanceMode::Auto {
            return;
        }
        apply_theme(&self.settings, window, cx);
        self.sync_background_controls(window, cx);
        cx.notify();
    }

    fn persist_destinations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.destination_input.read(cx).value().to_string();
        let paths = parse_destinations(&text);
        self.settings.destinations = paths.clone();
        self.queue
            .update(cx, |model, _| model.set_destinations(paths));
        self.save(window, cx);
        cx.notify();
    }

    fn sort_destinations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.destination_input.read(cx).value().to_string();
        let mut paths = parse_destinations(&text);
        if paths.len() < 2 {
            return;
        }
        paths.sort_by_key(|path| path.to_lowercase());
        let sorted = paths.join("\n");
        self.destination_input.update(cx, |input, cx| {
            input.set_value(sorted, window, cx);
        });
        self.persist_destinations(window, cx);
    }

    pub fn restart_wallpaper_shuffle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shuffle_task = None;
        if !self.settings.wallpaper_random {
            return;
        }

        let view = cx.entity().downgrade();
        self.shuffle_task = Some(cx.spawn_in(window, async move |_, cx| {
            loop {
                let duration = view
                    .update(cx, |this, _| this.settings.wallpaper_interval.duration())
                    .ok();
                let Some(duration) = duration else {
                    break;
                };
                cx.background_executor().timer(duration).await;
                if view
                    .update_in(cx, |this, window, cx| {
                        this.shuffle_wallpaper(window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn shuffle_wallpaper(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options: Vec<_> = self
            .wallpapers
            .iter()
            .enumerate()
            .filter(|(_, wallpaper)| matches!(wallpaper.source, WallpaperSource::User(_)))
            .collect();
        if options.len() < 2 {
            return;
        }

        let current = self.settings.wallpaper.as_deref();
        let current_index = options
            .iter()
            .position(|(_, wallpaper)| Some(wallpaper.id.as_str()) == current);
        let offset = random_index(options.len() - usize::from(current_index.is_some()));
        let position = match current_index {
            Some(index) => (index + 1 + offset) % options.len(),
            None => offset,
        };
        let (index, option) = options[position];
        self.settings.wallpaper = Some(option.id.clone());
        self.wallpaper_state.update(cx, |state, cx| {
            state.source = Some(option.source.clone());
            cx.notify();
        });
        self.wallpaper_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(index + 1)), window, cx);
        });
        let _ = self.settings.save();
        cx.notify();
    }

    fn reset_filename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.filename_template =
            crate::app_settings::DEFAULT_FILENAME_TEMPLATE.to_string();
        self.filename_input.update(cx, |input, cx| {
            input.set_value(self.settings.filename_template.clone(), window, cx)
        });
        self.save(window, cx);
        cx.notify();
    }

    fn model_settings(&self, cx: &App) -> DownloadSettings {
        *self.queue.read(cx).settings()
    }

    fn update_queue(&mut self, cx: &mut Context<Self>, update: impl FnOnce(&mut QueueModel)) {
        self.queue.update(cx, |model, _| update(model));
        cx.notify();
    }

    fn update_download_defaults(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut QueueModel),
        event: QueueEvent,
    ) {
        self.queue.update(cx, |model, cx| {
            update(model);
            cx.emit(event);
        });
        cx.notify();
    }

    fn save(&self, window: &mut Window, cx: &mut App) {
        if let Err(error) = self.settings.save() {
            window.push_notification(
                Notification::error(format!("設定を保存できませんでした: {error}")),
                cx,
            );
        }
    }

    fn set_appearance(
        &mut self,
        appearance: AppearanceMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.appearance = appearance;
        apply_theme(&self.settings, window, cx);
        self.sync_background_controls(window, cx);
        self.save(window, cx);
        cx.notify();
    }

    fn set_font_scale(&mut self, scale: FontScale, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.font_scale = scale;
        Theme::update(cx, |theme| theme.font_size = px(scale.px()));
        self.save(window, cx);
        cx.notify();
    }

    fn reset_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.background_color = None;
        apply_theme(&self.settings, window, cx);
        self.sync_background_controls(window, cx);
        self.save(window, cx);
        cx.notify();
    }

    fn current_background_hex(&self, cx: &App) -> String {
        self.settings
            .background_color
            .as_deref()
            .and_then(parse_hex)
            .map(hex_string)
            .unwrap_or_else(|| hex_string(cx.theme().background))
    }

    fn sync_background_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let color = self
            .settings
            .background_color
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(cx.theme().background);
        let hex = hex_string(color);
        self.color_picker
            .update(cx, |picker, cx| picker.set_value(color, window, cx));
        self.hex_input
            .update(cx, |input, cx| input.set_value(hex, window, cx));
        self.hex_invalid = false;
    }

    fn play_selected(
        &self,
        options: &[SoundOption],
        selected: &Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(option) = selected
            .as_deref()
            .and_then(|id| options.iter().find(|option| option.id == id))
            .or_else(|| options.first())
        else {
            window.push_notification(Notification::warning("再生できるサウンドがありません"), cx);
            return;
        };

        if let Err(error) = play_sound(&option.path) {
            window.push_notification(
                Notification::error(format!("サウンドを再生できませんでした: {error}")),
                cx,
            );
        }
    }

    fn selected_wallpaper(&self) -> Option<&WallpaperOption> {
        self.settings
            .wallpaper
            .as_deref()
            .and_then(|id| self.wallpapers.iter().find(|wallpaper| wallpaper.id == id))
    }

    fn wallpaper_preview(&self, cx: &App) -> AnyElement {
        let opacity = self.settings.wallpaper_opacity.clamp(0.0, 1.0);
        let Some(wallpaper) = self.selected_wallpaper() else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("背景無し"),
                )
                .into_any_element();
        };

        wallpaper_image(cx, &wallpaper.source, ObjectFit::Contain, opacity).unwrap_or_else(|| {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("画像を読み込めませんでした"),
                )
                .into_any_element()
        })
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.model_settings(cx);
        let audio_container_labels = Container::AUDIO
            .iter()
            .map(|container| container.label())
            .collect::<Vec<_>>()
            .join("、");
        let audio_container_note = format!(
            "映像なしでは音声用コンテナ（{audio_container_labels}）を使います。初期値は{}です。",
            DEFAULT_AUDIO_CONTAINER.label()
        );
        let sound_enabled = self.settings.play_sound;
        let preview_color = self
            .settings
            .background_color
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(cx.theme().background);
        let wallpaper_opacity = self.settings.wallpaper_opacity.clamp(0.0, 1.0);

        h_flex()
            .size_full()
            .justify_center()
            .child(
                div()
                    .id("settings-scroll")
                    .w_full()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_8()
                            .px_8()
                            .py_8()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(IconName::Settings2)
                                                    .size_5()
                                                    .text_color(cx.theme().primary),
                                            )
                                            .child(
                                                div()
                                                    .text_lg()
                                                    .font_semibold()
                                                    .child("設定"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("アプリ全体とダウンロードの共通設定です。"),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(section_title("変換終了時のアクション"))
                                    .child(
                                        settings_group(cx)
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        Checkbox::new("notify-on-complete")
                                                            .checked(
                                                                self.settings.notify_on_complete,
                                                            )
                                                            .label("OSの通知を表示する")
                                                            .on_change(cx.listener(
                                                                |this, checked, window, cx| {
                                                                    this.settings
                                                                        .notify_on_complete =
                                                                        *checked;
                                                                    this.save(window, cx);
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(info_popover(
                                                        "notify-info",
                                                        "OS内蔵の通知を表示します。通知の表示はOS側の権限設定が別途必要になります。",
                                                    )),
                                            )
                                            .child(
                                                Checkbox::new("play-sound")
                                                    .checked(self.settings.play_sound)
                                                    .label("サウンドを再生する")
                                                    .on_change(cx.listener(
                                                        |this, checked, window, cx| {
                                                            this.settings.play_sound = *checked;
                                                            this.save(window, cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(setting_row(
                                                "成功時のサウンド",
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div().flex_1().min_w_0().child(
                                                            Select::new(&self.success_select)
                                                                .w_full()
                                                                .disabled(
                                                                    !sound_enabled
                                                                        || self.settings.success_random,
                                                                ),
                                                        ),
                                                    )
                                                    .child(
                                                        Checkbox::new("success-random")
                                                            .label("ランダム")
                                                            .checked(self.settings.success_random)
                                                            .disabled(!sound_enabled)
                                                            .on_change(cx.listener(
                                                                |this, checked, window, cx| {
                                                                    this.settings.success_random =
                                                                        *checked;
                                                                    this.save(window, cx);
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("play-success")
                                                            .ghost()
                                                            .icon(IconName::Play)
                                                            .tooltip("成功時のサウンドを再生")
                                                            .accessibility_label(
                                                                "成功時のサウンドを再生",
                                                            )
                                                            .disabled(
                                                                !sound_enabled
                                                                    || self.settings.success_random,
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.play_selected(
                                                                        &this.success_sounds,
                                                                        &this.settings.success_sound,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                    ),
                                                Some(info_popover(
                                                    "success-sound-info",
                                                    ".config/tree-movie-downloader/sounds/success 配下に音声ファイルを配置すると、自分が用意したファイルを選択することができます。\nサンプル音声は「効果音ラボ https://soundeffect-lab.info/」を利用しています。",
                                                )),
                                            ))
                                            .child(setting_row(
                                                "失敗時のサウンド",
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div().flex_1().min_w_0().child(
                                                            Select::new(&self.error_select)
                                                                .w_full()
                                                                .disabled(
                                                                    !sound_enabled
                                                                        || self.settings.error_random,
                                                                ),
                                                        ),
                                                    )
                                                    .child(
                                                        Checkbox::new("error-random")
                                                            .label("ランダム")
                                                            .checked(self.settings.error_random)
                                                            .disabled(!sound_enabled)
                                                            .on_change(cx.listener(
                                                                |this, checked, window, cx| {
                                                                    this.settings.error_random =
                                                                        *checked;
                                                                    this.save(window, cx);
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("play-error")
                                                            .ghost()
                                                            .icon(IconName::Play)
                                                            .tooltip("失敗時のサウンドを再生")
                                                            .accessibility_label(
                                                                "失敗時のサウンドを再生",
                                                            )
                                                            .disabled(
                                                                !sound_enabled
                                                                    || self.settings.error_random,
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.play_selected(
                                                                        &this.error_sounds,
                                                                        &this.settings.error_sound,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                    ),
                                                Some(info_popover(
                                                    "error-sound-info",
                                                    ".config/tree-movie-downloader/sounds/error 配下に音声ファイルを配置すると、自分が用意したファイルを選択することができます。\nサンプル音声は「効果音ラボ https://soundeffect-lab.info/」を利用しています。",
                                                )),
                                            )),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(section_title("基本設定"))
                                    .child(
                                        settings_group(cx)
                                            .child(setting_row(
                                                "外観モード",
                                                RadioGroup::horizontal("appearance")
                                                    .children(
                                                        AppearanceMode::ALL
                                                            .map(AppearanceMode::label),
                                                    )
                                                    .selected_index(Some(
                                                        self.settings.appearance.index(),
                                                    ))
                                                    .on_change(cx.listener(
                                                        |this, index, window, cx| {
                                                            this.set_appearance(
                                                                AppearanceMode::from_index(*index),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "言語",
                                                v_flex()
                                                    .w_full()
                                                    .gap_1()
                                                    .child(
                                                        Select::new(&self.language_select)
                                                            .w_full(),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "表示言語の切り替えは今後対応します。",
                                                            ),
                                                    ),
                                                Some(info_popover(
                                                    "language-info",
                                                    "表示言語を切り替えます。画面の翻訳は今後追加します。",
                                                )),
                                            ))
                                            .child(setting_row(
                                                "文字の大きさ",
                                                RadioGroup::horizontal("font-scale")
                                                    .children(FontScale::ALL.map(FontScale::label))
                                                    .selected_index(Some(
                                                        self.settings.font_scale.index(),
                                                    ))
                                                    .on_change(cx.listener(
                                                        |this, index, window, cx| {
                                                            this.set_font_scale(
                                                                FontScale::from_index(*index),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "背景画像",
                                                Select::new(&self.wallpaper_select)
                                                    .w_full()
                                                    .disabled(self.settings.wallpaper_random),
                                                Some(info_popover(
                                                    "wallpaper-info",
                                                    ".config/tree-movie-downloader/wallpaper 配下に背景画像ファイルを配置すると、自分が用意したファイルを選択することができます。JPG・JPEG・PNG・WebP・JXLに対応しています。",
                                                )),
                                            ))
                                            .child(setting_row(
                                                "背景画像のシャッフル",
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div().w(px(200.)).child(
                                                            Select::new(&self.interval_select)
                                                                .w_full()
                                                                .disabled(
                                                                    !self
                                                                        .settings
                                                                        .wallpaper_random,
                                                                ),
                                                        ),
                                                    )
                                                    .child(
                                                        Checkbox::new("wallpaper-random")
                                                            .label("ランダム")
                                                            .checked(
                                                                self.settings.wallpaper_random,
                                                            )
                                                            .on_change(cx.listener(
                                                                |this, checked, window, cx| {
                                                                    this.settings.wallpaper_random =
                                                                        *checked;
                                                                    this.save(window, cx);
                                                                    this.restart_wallpaper_shuffle(
                                                                        window, cx,
                                                                    );
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "背景画像の不透明度",
                                                h_flex()
                                                    .w_full()
                                                    .gap_3()
                                                    .items_center()
                                                    .child(
                                                        div().flex_1().min_w_0().child(
                                                            Slider::new(&self.opacity_slider)
                                                                .horizontal()
                                                                .w_full(),
                                                        ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w_12()
                                                            .text_xs()
                                                            .text_right()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(format!(
                                                                "{:.0}%",
                                                                wallpaper_opacity * 100.0
                                                            )),
                                                    ),
                                                Some(info_popover(
                                                    "opacity-info",
                                                    "背景画像の不透明度を調節できます。100%で背景画像がそのまま表示されます。",
                                                )),
                                            ))
                                            .child(setting_row_top(
                                                "プレビュー",
                                                div()
                                                    .relative()
                                                    .w_full()
                                                    .h(px(160.))
                                                    .rounded(cx.theme().radius_lg)
                                                    .overflow_hidden()
                                                    .border_1()
                                                    .border_color(cx.theme().border)
                                                    .bg(preview_color)
                                                    .child(self.wallpaper_preview(cx)),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "背景色",
                                                v_flex()
                                                    .w_full()
                                                    .gap_1()
                                                    .child(
                                                        h_flex()
                                                            .w_full()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                ColorPicker::new(&self.color_picker)
                                                                    .accessibility_label("背景色"),
                                                            )
                                                            .child(
                                                                div().w(px(120.)).child(
                                                                    Input::new(&self.hex_input)
                                                                        .aria_label("背景色の16進数")
                                                                        .border_color(
                                                                            if self.hex_invalid {
                                                                                cx.theme().danger
                                                                            } else {
                                                                                cx.theme().input
                                                                            },
                                                                        )
                                                                        .w_full(),
                                                                ),
                                                            )
                                                            .child(
                                                                Button::new("reset-background")
                                                                    .ghost()
                                                                    .icon(IconName::RotateCcw)
                                                                    .tooltip("背景色を初期値に戻す")
                                                                    .accessibility_label(
                                                                        "背景色を初期値に戻す",
                                                                    )
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.reset_background(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            ),
                                                    )
                                                    .when(self.hex_invalid, |this| {
                                                        this.child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(cx.theme().danger)
                                                                .child(
                                                                    "色の形式が正しくありません（例: #FFFFFF）",
                                                                ),
                                                        )
                                                    }),
                                                Some(info_popover(
                                                    "background-color-info",
                                                    "背景画像は縦横比を維持するため、余白ができることがあります。その余白部分の色が背景色になります。",
                                                )),
                                            ))
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(section_title("ダウンロード"))
                                    .child(
                                        settings_group(cx)
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        "画質と音質は、一括ダウンロード画面の各行のメニュー、URL追加ダイアログ、かんたんダウンロードの詳細設定で変更できます。",
                                                    ),
                                            )
                                            .child(setting_row(
                                                "ファイル名ルール",
                                                v_flex()
                                                    .w_full()
                                                    .gap_1()
                                                    .child(
                                                        h_flex()
                                                            .w_full()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                div().flex_1().min_w_0().child(
                                                                    Input::new(&self.filename_input)
                                                                        .aria_label("ファイル名ルール")
                                                                        .w_full(),
                                                                ),
                                                            )
                                                            .child(
                                                                Button::new("reset-filename")
                                                                    .small()
                                                                    .ghost()
                                                                    .icon(IconName::RotateCcw)
                                                                    .label("リセット")
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.reset_filename(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_start()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    )
                                                                    .child("プレビュー"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .min_w_0()
                                                                    .text_xs()
                                                                    .font_family(
                                                                        cx.theme()
                                                                            .mono_font_family
                                                                            .clone(),
                                                                    )
                                                                    .child(preview_filename(
                                                                        &self.settings
                                                                            .filename_template,
                                                                    )),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "プレビューはサンプル値です。実際のダウンロード時にはyt-dlpの出力テンプレートへ変換します。",
                                                            ),
                                                    ),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "変数一覧",
                                                filename_variables_table(cx),
                                                None,
                                            ))
                                            .child(setting_row_top(
                                                "ダウンロード初期値",
                                                v_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .child(download_default_row(
                                                        "画質",
                                                        RadioGroup::horizontal("quality")
                                                            .children(DOWNLOAD_QUALITY_CHOICES.map(
                                                                |quality| {
                                                                    download_default_radio(
                                                                        quality.label(),
                                                                        None,
                                                                    )
                                                                },
                                                            ))
                                                            .selected_index(Some(
                                                                DOWNLOAD_QUALITY_CHOICES
                                                                    .iter()
                                                                    .position(|quality| {
                                                                        *quality == settings.quality()
                                                                    })
                                                                    .unwrap_or(0),
                                                            ))
                                                            .on_change(cx.listener(
                                                                |this, index, window, cx| {
                                                                    let quality: Quality =
                                                                        DOWNLOAD_QUALITY_CHOICES[*index];
                                                                    this.settings.quality_index =
                                                                        quality.index();
                                                                    this.update_download_defaults(
                                                                        cx,
                                                                        |model| {
                                                                            model.set_quality(quality);
                                                                        },
                                                                         QueueEvent::QualityDefaultChanged(quality),
                                                                    );
                                                                    this.save(window, cx);
                                                                },
                                                            )),
                                                    ))
                                                    .child(download_default_row(
                                                        "音質",
                                                        RadioGroup::horizontal("audio")
                                                            .children(AudioQuality::ALL.map(|audio| {
                                                                download_default_radio(
                                                                    audio.label(),
                                                                    None,
                                                                )
                                                            }))
                                                            .selected_index(Some(
                                                                settings.audio().index(),
                                                            ))
                                                            .on_change(cx.listener(
                                                                |this, index, window, cx| {
                                                                    let audio =
                                                                        AudioQuality::from_index(*index);
                                                                    this.settings.audio_index = audio.index();
                                                                    this.update_download_defaults(
                                                                        cx,
                                                                        |model| {
                                                                            model.set_audio(audio);
                                                                        },
                                                                         QueueEvent::AudioDefaultChanged(audio),
                                                                    );
                                                                    this.save(window, cx);
                                                                },
                                                            )),
                                                    ))
                                                    .child(download_default_row(
                                                        "コンテナ",
                                                        RadioGroup::horizontal("container")
                                                            .disabled(
                                                                settings.quality()
                                                                    == Quality::AudioOnly,
                                                            )
                                                            .children(Container::VIDEO.map(
                                                                |container| {
                                                                    let strength = match container {
                                                                        Container::Mp4 => MP4_STRENGTH,
                                                                        Container::Mkv => MKV_STRENGTH,
                                                                        Container::Webm => WEBM_STRENGTH,
                                                                        _ => WEBM_STRENGTH,
                                                                    };
                                                                    download_default_radio(
                                                                        container.label(),
                                                                        Some(strength),
                                                                    )
                                                                },
                                                            ))
                                                            .selected_index(Some(
                                                                Container::VIDEO
                                                                    .iter()
                                                                    .position(|container| {
                                                                        *container == settings.container()
                                                                    })
                                                                    .unwrap_or(0),
                                                            ))
                                                            .on_change(cx.listener(
                                                                |this, index, window, cx| {
                                                                    let container: Container =
                                                                        Container::VIDEO[*index];
                                                                    this.settings.container_index =
                                                                        container.index();
                                                                    this.update_download_defaults(
                                                                        cx,
                                                                        |model| {
                                                                            model.set_container(container);
                                                                        },
                                                                         QueueEvent::ContainerDefaultChanged(container),
                                                                    );
                                                                    this.save(window, cx);
                                                                },
                                                            )),
                                                    ))
                                                    .when(
                                                        settings.quality() == Quality::AudioOnly,
                                                        |this| {
                                                            this.child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    )
                                                                    .child(
                                                                        audio_container_note,
                                                                    ),
                                                            )
                                                        },
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "URLを追加する際の初期値を上記に設定します。",
                                                            ),
                                                    ),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "保存先",
                                                v_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .child(
                                                        Textarea::new(&self.destination_input)
                                                            .aria_label("保存先フォルダ")
                                                            .w_full()
                                                            .h(px(140.)),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                Button::new("sort-destinations")
                                                                    .small()
                                                                    .ghost()
                                                                    .icon(IconName::ArrowDownAZ)
                                                                    .label("パス順に並べ替え")
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.sort_destinations(
                                                                                window, cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "1行に1件、保存先フォルダのフルパスを入力してください。OS標準のダウンロードフォルダは常に先頭に表示され、この欄には表示されません。",
                                                            ),
                                                    ),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "自動リトライ",
                                                v_flex()
                                                    .w_full()
                                                    .gap_1()
                                                    .child(
                                                        RadioGroup::horizontal("retries")
                                                            .children(RETRY_LABELS)
                                                            .selected_index(Some(
                                                                settings.retry_index(),
                                                            ))
                                                            .on_change(cx.listener(
                                                                |this, index, window, cx| {
                                                                    let retries = RETRY_CHOICES
                                                                        .get(*index)
                                                                        .copied()
                                                                        .unwrap_or(3);
                                                                    this.settings.retries = retries;
                                                                    this.update_queue(cx, |model| {
                                                                        model.set_retries(retries);
                                                                    });
                                                                    this.save(window, cx);
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(format!(
                                                                "待機時間を空けて、最大{}回まで再試行します。",
                                                                settings.retries()
                                                            )),
                                                    ),
                                                None,
                                            ))
                                            .child(setting_row(
                                                "ブラウザのCookie",
                                                v_flex()
                                                    .w_full()
                                                    .gap_1()
                                                    .child(
                                                        Select::new(&self.cookie_select).w_full(),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .child(
                                                                "YouTubeで「ログインして確認」と表示されたときに使います。ログイン済みのブラウザを選んでください。",
                                                            ),
                                                    ),
                                                Some(info_popover(
                                                    "cookie-info",
                                                    "ブラウザに保存されたCookieをyt-dlpへ渡します。macOSの権限により読み取れない場合は、環境変数 YT_DLP_COOKIES_FROM_BROWSER も利用できます。Safariはフルディスクアクセスが必要です。",
                                                )),
                                            ))
                                    ),
                            ),
                    ),
            )
    }
}

fn parse_destinations(text: &str) -> Vec<String> {
    let default = default_downloads_dir();
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line == default || paths.iter().any(|path| path == line) {
            continue;
        }
        paths.push(line.to_string());
    }
    paths
}

fn random_index(bound: usize) -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize % bound)
        .unwrap_or(0)
}

fn apply_theme(settings: &AppSettings, window: &mut Window, cx: &mut App) {
    let mode = match settings.appearance {
        AppearanceMode::Auto => match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        },
        AppearanceMode::Light => ThemeMode::Light,
        AppearanceMode::Dark => ThemeMode::Dark,
    };
    Theme::change(mode, Some(window), cx);
    let background = settings
        .background_color
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or_else(|| default_background(mode));
    Theme::update(cx, |theme| {
        theme.font_size = px(settings.font_scale.px());
        theme.colors.background = background;
    });
}

fn default_background(mode: ThemeMode) -> Hsla {
    parse_hex(if mode.is_dark() { "#1E1E1E" } else { "#FFFFFF" })
        .expect("default background is a valid hex color")
}

fn selected_sound_index(selected: Option<&str>, options: &[SoundOption]) -> Option<usize> {
    selected
        .and_then(|id| options.iter().position(|option| option.id == id))
        .or_else(|| (!options.is_empty()).then_some(0))
}

fn section_title(title: &'static str) -> Div {
    div().text_sm().font_semibold().child(title)
}

fn download_default_row(label: &'static str, radios: RadioGroup) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .child(div().w(px(80.)).flex_shrink_0().text_sm().child(label))
        .child(radios)
}

fn download_default_radio(label: &'static str, strength: Option<&'static str>) -> Radio {
    let radio = Radio::new(label).w(px(120.));

    if let Some(strength) = strength {
        radio.accessibility_label(label).tooltip(strength).child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().child(label))
                .child(Icon::new(IconName::Info).xsmall()),
        )
    } else {
        radio.label(label)
    }
}

fn setting_row(
    label: &'static str,
    controls: impl IntoElement + 'static,
    info: Option<HoverCard>,
) -> AnyElement {
    let mut row = h_flex()
        .w_full()
        .items_center()
        .gap_4()
        .child(div().w(px(160.)).flex_shrink_0().text_sm().child(label))
        .child(div().flex_1().min_w_0().child(controls));
    if let Some(info) = info {
        row = row.child(info);
    }
    row.into_any_element()
}

fn info_popover(id: &'static str, text: &'static str) -> HoverCard {
    HoverCard::new(id)
        .trigger(
            Button::new(SharedString::from(format!("info-trigger-{id}")))
                .ghost()
                .small()
                .icon(IconName::Info)
                .tooltip("説明を表示")
                .accessibility_label("説明を表示"),
        )
        .content(move |_, _, cx| {
            div()
                .max_w(px(340.))
                .p_3()
                .text_xs()
                .text_color(cx.theme().popover_foreground)
                .child(text)
        })
}

fn settings_group(cx: &App) -> GroupBox {
    GroupBox::new().fill().content_style(
        StyleRefinement::default()
            .border_1()
            .border_color(cx.theme().border),
    )
}

fn setting_row_top(
    label: &'static str,
    controls: impl IntoElement + 'static,
    info: Option<HoverCard>,
) -> AnyElement {
    let mut row = h_flex()
        .w_full()
        .items_start()
        .gap_4()
        .child(div().w(px(160.)).flex_shrink_0().text_sm().child(label))
        .child(div().flex_1().min_w_0().child(controls));
    if let Some(info) = info {
        row = row.child(info);
    }
    row.into_any_element()
}

fn filename_variables_table(cx: &App) -> AnyElement {
    let header = h_flex()
        .w_full()
        .bg(cx.theme().muted.opacity(0.45))
        .child(
            div()
                .w(px(160.))
                .px_2()
                .py_1()
                .text_xs()
                .font_semibold()
                .child("変数名"),
        )
        .child(
            div()
                .flex_1()
                .px_2()
                .py_1()
                .text_xs()
                .font_semibold()
                .child("意味"),
        );

    v_flex()
        .w_full()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_md()
        .overflow_hidden()
        .child(header)
        .children(
            FILENAME_VARIABLES
                .iter()
                .enumerate()
                .map(|(index, (name, meaning))| {
                    h_flex()
                        .w_full()
                        .when(index + 1 < FILENAME_VARIABLES.len(), |this| {
                            this.border_b_1().border_color(cx.theme().border)
                        })
                        .child(
                            div()
                                .w(px(160.))
                                .px_2()
                                .py_1()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(format!("${{{name}}}")),
                        )
                        .child(div().flex_1().px_2().py_1().text_xs().child(*meaning))
                        .into_any_element()
                }),
        )
        .into_any_element()
}

fn preview_filename(template: &str) -> String {
    let mut filename = template.to_string();
    let values = [
        ("uploadDate", "20260907"),
        ("title", "サンプル動画"),
        ("duration", "02h43m04s"),
        ("width", "1280"),
        ("height", "720"),
        ("vcodec", "av01.0.08M.08"),
        ("fps", "60FPS"),
        ("acodec", "opus"),
        ("abr", "136k"),
        ("id", "ZqSsDmJI-3s"),
        ("ext", "mp4"),
    ];
    for (name, value) in values {
        filename = filename.replace(&format!("${{{name}}}"), value);
    }
    if filename.trim().is_empty() {
        "（ファイル名が空です）".to_string()
    } else {
        filename
    }
}

#[cfg(test)]
mod tests {
    use super::parse_destinations;
    use crate::model::default_downloads_dir;

    #[test]
    fn parses_destination_lines() {
        let default = default_downloads_dir();
        let text = format!("/tmp/a\n\n {default} \n/tmp/b\n/tmp/a\n");

        assert_eq!(parse_destinations(&text), vec!["/tmp/a", "/tmp/b"]);
    }
}
