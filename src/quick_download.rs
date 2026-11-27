//! 動画URLを1件入力してすぐにダウンロードする画面。

use std::path::Path;

use gpui_kit::AppContext as _;
use gpui_kit::Focusable as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Selectable as _, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    notification::Notification,
    progress::Progress,
    scroll::ScrollableElement as _,
    select::{Select, SelectEvent, SelectState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{Context, Entity, EventEmitter, Render, SharedString, Subscription, Window, div};

use crate::downloader::DownloadManager;
use crate::file_actions::{open_output_file, open_output_folder};
use crate::model::{
    AudioQuality, Container, DEFAULT_AUDIO, DEFAULT_AUDIO_CONTAINER, DEFAULT_CONTAINER,
    DEFAULT_QUALITY, ItemStatus, Quality, QueueEvent, QueueItem, QueueModel, compact_url,
    container_index, resolve_container,
};
use crate::quick_url::{QuickUrlError, validate_quick_url};
use crate::update_controller::UpdateController;
use crate::video_preview::{PreviewSize, VideoPreview};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuickDownloadEvent {
    OpenBatch,
    OpenSettings,
}

impl EventEmitter<QuickDownloadEvent> for QuickDownloadView {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickAction {
    Start,
    Redownload,
    Cancel,
    Retry,
    Resume,
}

impl QuickAction {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "ダウンロード",
            Self::Redownload => "再ダウンロード",
            Self::Cancel => "キャンセル",
            Self::Retry => "再試行",
            Self::Resume => "開始",
        }
    }
}

pub struct QuickDownloadView {
    queue: Entity<QueueModel>,
    downloads: Entity<DownloadManager>,
    updates: Entity<UpdateController>,
    url_input: Entity<InputState>,
    destination: Entity<SelectState<Vec<SharedString>>>,
    quality: Entity<SelectState<Vec<&'static str>>>,
    audio: Entity<SelectState<Vec<&'static str>>>,
    container: Entity<SelectState<Vec<&'static str>>>,
    details_open: bool,
    error_expanded: bool,
    job_id: Option<u64>,
    validation_error: Option<QuickUrlError>,
    action_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl QuickDownloadView {
    pub fn new(
        queue: Entity<QueueModel>,
        downloads: Entity<DownloadManager>,
        updates: Entity<UpdateController>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://www.youtube.com/watch?v=...")
        });
        let destination_labels: Vec<SharedString> = queue
            .read(cx)
            .destinations()
            .iter()
            .map(SharedString::from)
            .collect();
        let destination =
            cx.new(|cx| SelectState::new(destination_labels, Some(IndexPath::new(0)), window, cx));
        let quality = cx.new(|cx| {
            SelectState::new(
                Quality::ALL.map(Quality::label).to_vec(),
                Some(IndexPath::new(DEFAULT_QUALITY.index())),
                window,
                cx,
            )
        });
        let audio = cx.new(|cx| {
            SelectState::new(
                AudioQuality::ALL.map(AudioQuality::label).to_vec(),
                Some(IndexPath::new(DEFAULT_AUDIO.index())),
                window,
                cx,
            )
        });
        let settings_container = queue.read(cx).settings().container();
        let selected_container_index =
            container_index(&Container::VIDEO, settings_container, DEFAULT_CONTAINER);
        let container = cx.new(|cx| {
            SelectState::new(
                Container::VIDEO.map(Container::label).to_vec(),
                Some(IndexPath::new(selected_container_index)),
                window,
                cx,
            )
        });

        let queue_subscription = cx.observe(&queue, |_, _, cx| cx.notify());
        let updates = updates.clone();
        let update_subscription = cx.observe(&updates, |_, _, cx| cx.notify());
        let input_subscription =
            cx.subscribe_in(&url_input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.validation_error = None;
                    this.action_error = None;
                    this.error_expanded = false;
                    cx.notify();
                }
            });
        let quality_subscription = cx.subscribe_in(
            &quality,
            window,
            |this, _, event: &SelectEvent<Vec<&'static str>>, window, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                let quality = Quality::ALL
                    .iter()
                    .copied()
                    .find(|quality| quality.label() == *label)
                    .unwrap_or(DEFAULT_QUALITY);
                this.sync_container(quality, window, cx);
            },
        );

        Self {
            queue,
            downloads,
            updates,
            url_input,
            destination,
            quality,
            audio,
            container,
            details_open: false,
            error_expanded: false,
            job_id: None,
            validation_error: None,
            action_error: None,
            _subscriptions: vec![
                queue_subscription,
                update_subscription,
                input_subscription,
                quality_subscription,
            ],
        }
    }

    pub fn focus_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.url_input.focus_handle(cx).focus(window, cx);
    }

    pub fn sync_destinations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let labels: Vec<SharedString> = self
            .queue
            .read(cx)
            .destinations()
            .iter()
            .map(SharedString::from)
            .collect();
        let selected = self
            .destination
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|selected| labels.iter().position(|label| *label == selected))
            .unwrap_or(0);
        self.destination.update(cx, |state, cx| {
            state.set_items(labels, window, cx);
            state.set_selected_index(Some(IndexPath::new(selected)), window, cx);
        });
    }

    fn current_job(&self, cx: &Context<Self>) -> Option<QueueItem> {
        self.job_id
            .and_then(|id| self.queue.read(cx).item(id).cloned())
    }

    fn action(&self, cx: &Context<Self>) -> QuickAction {
        let Some(job) = self.current_job(cx) else {
            return QuickAction::Start;
        };
        if job.status() == ItemStatus::Downloading {
            return QuickAction::Cancel;
        }
        let url = self.url_input.read(cx).value();
        if job.url().as_ref() == url.trim() {
            match job.status() {
                ItemStatus::Queued => QuickAction::Resume,
                ItemStatus::Failed | ItemStatus::Paused => QuickAction::Retry,
                ItemStatus::Completed | ItemStatus::Skipped => QuickAction::Redownload,
                _ => QuickAction::Start,
            }
        } else if redownload_id(self.queue.read(cx), url.trim()).is_some() {
            QuickAction::Redownload
        } else {
            QuickAction::Start
        }
    }

    pub fn start_or_cancel(&mut self, updating: bool, cx: &mut Context<Self>) {
        match self.action(cx) {
            QuickAction::Cancel => {
                let mut cancel_error = None;
                if let Some(id) = self.job_id {
                    let result = self
                        .downloads
                        .update(cx, |manager, cx| manager.cancel_item(id, cx));
                    if let Err(error) = result {
                        cancel_error = Some(error);
                    }
                }
                self.action_error = cancel_error;
                cx.notify();
            }
            QuickAction::Retry | QuickAction::Resume => self.retry_current(updating, cx),
            QuickAction::Start | QuickAction::Redownload => self.start(updating, cx),
        }
    }

    pub fn retry_current(&mut self, updating: bool, cx: &mut Context<Self>) {
        let Some(id) = self.job_id else {
            return;
        };
        let retryable = self.queue.read(cx).item(id).is_some_and(|item| {
            matches!(
                item.status(),
                ItemStatus::Queued | ItemStatus::Failed | ItemStatus::Paused
            )
        });
        if !retryable {
            return;
        }
        if updating || self.queue.read(cx).active_item().is_some() {
            cx.notify();
            return;
        }
        self.validation_error = None;
        self.action_error = None;
        self.queue.update(cx, |model, cx| {
            model.retry(id);
            cx.emit(QueueEvent::Changed);
            cx.notify();
        });
        if !self
            .downloads
            .update(cx, |manager, cx| manager.start_item(id, cx))
        {
            self.action_error =
                Some("開始できませんでした。一括ダウンロードから開始してください。".into());
        }
        self.error_expanded = false;
        cx.notify();
    }

    fn start(&mut self, updating: bool, cx: &mut Context<Self>) {
        let value = self.url_input.read(cx).value();
        let url = match validate_quick_url(&value) {
            Ok(url) => url,
            Err(error) => {
                self.validation_error = Some(error);
                self.action_error = None;
                cx.notify();
                return;
            }
        };
        if updating || self.queue.read(cx).active_item().is_some() {
            cx.notify();
            return;
        }
        let redownload_id = if self.queue.read(cx).has_url(&url) {
            let id = redownload_id(self.queue.read(cx), &url);
            let Some(id) = id else {
                self.validation_error = None;
                self.action_error = Some("このURLはすでにキューに追加されています。".into());
                cx.notify();
                return;
            };
            Some(id)
        } else {
            None
        };

        let quality = self
            .quality
            .read(cx)
            .selected_index(cx)
            .map(|index| Quality::from_index(index.row))
            .unwrap_or(DEFAULT_QUALITY);
        let audio = self
            .audio
            .read(cx)
            .selected_index(cx)
            .map(|index| AudioQuality::from_index(index.row))
            .unwrap_or(DEFAULT_AUDIO);
        let container = self
            .container
            .read(cx)
            .selected_index(cx)
            .and_then(|index| Container::options_for(quality).get(index.row).copied())
            .unwrap_or(if quality == Quality::AudioOnly {
                DEFAULT_AUDIO_CONTAINER
            } else {
                DEFAULT_CONTAINER
            });
        let destination = self
            .destination
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.queue.read(cx).destinations().get(index.row).cloned())
            .unwrap_or_default();

        let redownload = redownload_id.is_some();
        let id = self.queue.update(cx, |model, cx| {
            let id = match redownload_id {
                Some(id) => model.replace_completed_single_url(
                    id,
                    &url,
                    quality,
                    audio,
                    container,
                    &destination,
                )?,
                None => model.add_single_url(&url, quality, audio, container, &destination)?,
            };
            cx.emit(QueueEvent::Changed);
            cx.notify();
            Some(id)
        });
        let Some(id) = id else {
            self.action_error = Some("URLをキューに追加できませんでした。".into());
            cx.notify();
            return;
        };
        self.job_id = Some(id);
        self.validation_error = None;
        self.action_error = None;
        self.error_expanded = false;

        let started = if redownload {
            self.downloads
                .update(cx, |manager, cx| manager.start_forced_item(id, cx))
        } else {
            self.downloads
                .update(cx, |manager, cx| manager.start_item(id, cx))
        };
        if !started {
            if redownload {
                self.job_id = Some(id);
                self.action_error = Some(
                    "再ダウンロードを開始できませんでした。キューに残しているため、後から開始できます。"
                        .into(),
                );
            } else {
                self.queue.update(cx, |model, cx| {
                    model.remove(id);
                    cx.emit(QueueEvent::Changed);
                    cx.notify();
                });
                self.job_id = None;
                self.action_error =
                    Some("ダウンロードを開始できませんでした。もう一度お試しください。".into());
            }
        }
        cx.notify();
    }

    fn sync_container(&mut self, quality: Quality, window: &mut Window, cx: &mut Context<Self>) {
        let options = Container::options_for(quality);
        let settings_container = self.queue.read(cx).settings().container();
        let current = self
            .container
            .read(cx)
            .selected_value()
            .copied()
            .and_then(|label| {
                Container::ALL
                    .iter()
                    .copied()
                    .find(|container| container.label() == label)
            });
        let selected = resolve_container(quality, current, settings_container);
        let selected = options
            .iter()
            .position(|container| *container == selected)
            .unwrap_or(0);
        let labels: Vec<&'static str> = options.iter().map(|container| container.label()).collect();
        self.container.update(cx, |state, cx| {
            state.set_items(labels, window, cx);
            state.set_selected_index(Some(IndexPath::new(selected)), window, cx);
        });
    }

    fn render_page(&self, updating: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let job = self.current_job(cx);
        let busy = job
            .as_ref()
            .is_some_and(|item| item.status() == ItemStatus::Downloading);
        let input_url = self.url_input.read(cx).value();
        let retrying_same_url = job.as_ref().is_some_and(|item| {
            matches!(
                item.status(),
                ItemStatus::Queued | ItemStatus::Failed | ItemStatus::Paused
            ) && item.url().as_ref() == input_url.trim()
        });
        let settings_disabled = busy || retrying_same_url;
        let other_running = !busy && self.queue.read(cx).active_item().is_some();
        let action = self.action(cx);
        let validation_error = self.validation_error;
        let action_error = self.action_error.clone();
        let details_open = self.details_open;
        let max_width = cx.theme().font_size * 45.0;

        v_flex()
            .size_full()
            .items_center()
            .overflow_y_scrollbar()
            .id("quick-scroll")
            .child(
                v_flex()
                    .w_full()
                    .max_w(max_width)
                    .p_8()
                    .gap_6()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .child("かんたんダウンロード"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("動画URLを1件貼り付けて、保存先を確認するだけで始められます。"),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().font_medium().child("URL"))
                            .child(
                                Input::new(&self.url_input)
                                    .id("quick-url")
                                    .aria_label("動画URL")
                                    .content_type(InputContentType::Url)
                                    .w_full()
                                    .disabled(busy),
                            )
                            .when_some(validation_error, |this, error| {
                                this.child(
                                    h_flex()
                                        .items_start()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_sm()
                                                .text_color(cx.theme().danger)
                                                .child(error.message()),
                                        )
                                        .when(error == QuickUrlError::Collection, |row| {
                                            row.child(
                                                Button::new("quick-open-batch")
                                                    .small()
                                                    .outline()
                                                    .label("一括ダウンロードを開く")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        cx.emit(QuickDownloadEvent::OpenBatch);
                                                        this.validation_error = None;
                                                        cx.notify();
                                                    })),
                                            )
                                        }),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().font_medium().child("保存先"))
                            .child(
                                Select::new(&self.destination)
                                    .id("quick-destination")
                                    .accessibility_label("保存先")
                                    .w_full()
                                    .disabled(settings_disabled),
                            ),
                    )
                    .child(
                        Button::new("quick-details")
                            .ghost()
                            .selected(details_open)
                            .toggled(details_open)
                            .icon(if details_open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .label("詳細設定")
                            .disabled(settings_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.details_open = !this.details_open;
                                cx.notify();
                            })),
                    )
                    .when(details_open, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_3()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("画質"))
                                        .child(
                                            Select::new(&self.quality)
                                                .id("quick-quality")
                                                .accessibility_label("画質")
                                                .w_full()
                                                .disabled(settings_disabled),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("音質"))
                                        .child(
                                            Select::new(&self.audio)
                                                .id("quick-audio")
                                                .accessibility_label("音質")
                                                .w_full()
                                                .disabled(settings_disabled),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("コンテナ"))
                                        .child(
                                            Select::new(&self.container)
                                                .id("quick-container")
                                                .accessibility_label("コンテナ")
                                                .w_full()
                                                .disabled(settings_disabled),
                                        ),
                                ),
                        )
                    })
                    .when_some(action_error, |this, error| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .when(other_running || updating, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(if updating {
                                    "更新処理中はダウンロードを開始できません。"
                                } else {
                                    "別のダウンロードが実行中です。完了後に開始してください。"
                                }),
                        )
                    })
                    .child(
                        Button::new("quick-start")
                            .primary()
                            .large()
                            .font_semibold()
                            .w_full()
                            .label(action.label())
                            .tooltip(if updating {
                                "更新処理中は開始できません"
                            } else if other_running {
                                "別のダウンロードが実行中です"
                            } else {
                                "⌘Enter"
                            })
                            .disabled(updating || other_running)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_or_cancel(updating, cx);
                            })),
                    )
                    .child(self.render_job(job.as_ref(), cx)),
            )
            .into_any_element()
    }

    fn render_job(&self, job: Option<&QueueItem>, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(job) = job else {
            return v_flex()
                .p_4()
                .gap_1()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .text_color(cx.theme().muted_foreground)
                        .child("ダウンロードすると、ここに進捗が表示されます。"),
                )
                .into_any_element();
        };

        let status = job.status();
        let progress = job.progress().clamp(0.0, 100.0);
        let mut panel = v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(compact_url(job.url())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(status_color(status, cx))
                            .child(status_label(job)),
                    ),
            );

        if status == ItemStatus::Downloading {
            panel = panel
                .child(
                    Progress::new(("quick-progress", job.id()))
                        .value(progress)
                        .loading(job.is_total_unknown())
                        .color(cx.theme().primary),
                )
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{progress:.0}%"))
                        .child(format!("{} / {}", job.downloaded(), job.total()))
                        .when_some(job.eta().cloned(), |this, eta| this.child(eta)),
                );
        }

        if status == ItemStatus::Failed {
            let error = job
                .error()
                .cloned()
                .unwrap_or_else(|| "ダウンロードに失敗しました。".into());
            let short_error = error.lines().next().unwrap_or(&error).to_string();
            panel = panel.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(short_error),
            );
            if error.contains('\n') {
                panel = panel.child(
                    Button::new("quick-error-details")
                        .xsmall()
                        .ghost()
                        .label(if self.error_expanded {
                            "詳細を隠す"
                        } else {
                            "詳細を表示"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error_expanded = !this.error_expanded;
                            cx.notify();
                        })),
                );
            }
            if self.error_expanded {
                panel = panel.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(error.clone()),
                );
            }
            if requires_authentication(&error) {
                panel = panel.child(
                    Button::new("quick-open-settings")
                        .small()
                        .outline()
                        .icon(IconName::Settings2)
                        .label("ブラウザのCookieを設定")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(QuickDownloadEvent::OpenSettings);
                            this.error_expanded = false;
                            cx.notify();
                        })),
                );
            }
        }

        if matches!(status, ItemStatus::Completed | ItemStatus::Skipped) {
            if let Some(preview) = VideoPreview::from_item(job, PreviewSize::Card) {
                panel = panel.child(preview);
            }
            let output_path = job.output_path().map(str::to_string);
            let folder = output_path
                .as_deref()
                .and_then(|path| Path::new(path).parent())
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| job.destination().to_string());
            let mut actions = h_flex().gap_2();
            if let Some(path) = output_path {
                let play_path = path.clone();
                actions = actions.child(
                    Button::new(("quick-play", job.id()))
                        .icon(IconName::Play)
                        .label("再生")
                        .on_click(move |_, window, cx| {
                            if let Err(error) = open_output_file(&play_path) {
                                window.push_notification(
                                    Notification::error(format!(
                                        "ファイルを再生できませんでした: {error}"
                                    )),
                                    cx,
                                );
                            }
                        }),
                );
            }
            actions = actions.child(
                Button::new(("quick-open-folder", job.id()))
                    .icon(IconName::FolderOpen)
                    .label("保存先を開く")
                    .on_click(move |_, window, cx| {
                        if let Err(error) = open_output_folder(&folder) {
                            window.push_notification(
                                Notification::error(format!("保存先を開けませんでした: {error}")),
                                cx,
                            );
                        }
                    }),
            );
            panel = panel.child(actions);
        }

        panel.into_any_element()
    }
}

impl Render for QuickDownloadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updating = {
            let updates = self.updates.read(cx);
            updates.is_installing() || updates.is_library_installing()
        };
        self.render_page(updating, cx)
    }
}

fn redownload_id(model: &QueueModel, url: &str) -> Option<u64> {
    model
        .items()
        .iter()
        .find(|item| {
            item.url().as_ref() == url.trim()
                && matches!(item.status(), ItemStatus::Completed | ItemStatus::Skipped)
        })
        .map(|item| item.id())
}

fn status_color(status: ItemStatus, cx: &gpui_kit::App) -> gpui_kit::Hsla {
    match status {
        ItemStatus::Completed => cx.theme().success,
        ItemStatus::Failed => cx.theme().danger,
        ItemStatus::Paused => cx.theme().warning,
        ItemStatus::Skipped => cx.theme().muted_foreground,
        _ => cx.theme().primary,
    }
}

fn status_label(job: &QueueItem) -> &'static str {
    match job.status() {
        ItemStatus::Queued => "開始待ち",
        ItemStatus::Downloading if job.progress() <= 0.0 && job.is_downloaded_unknown() => "取得中",
        ItemStatus::Downloading if job.progress() >= 100.0 => "変換中",
        ItemStatus::Downloading => "ダウンロード中",
        ItemStatus::Paused => "一時停止",
        ItemStatus::Completed => "完了",
        ItemStatus::Failed => "失敗",
        ItemStatus::Skipped => "保存済み",
    }
}

fn requires_authentication(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("認証")
        || error.contains("sign in")
        || error.contains("not a bot")
        || error.contains("cookies")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DEFAULT_AUDIO, DEFAULT_CONTAINER, DEFAULT_QUALITY};

    fn job(status: ItemStatus, url: &str) -> QueueItem {
        let mut model = QueueModel::new();
        let id = model
            .add_single_url(
                url,
                DEFAULT_QUALITY,
                DEFAULT_AUDIO,
                DEFAULT_CONTAINER,
                "/tmp",
            )
            .expect("add URL");
        let site = model.item(id).expect("job").site();
        match status {
            ItemStatus::Downloading => model.take_item_for_site(site, id).expect("start"),
            ItemStatus::Failed => {
                model.fail_item(id, "failed");
                model.item(id).expect("job").clone()
            }
            ItemStatus::Paused => {
                model.take_item_for_site(site, id).expect("start");
                model.pause_item(id);
                model.item(id).expect("job").clone()
            }
            ItemStatus::Completed => {
                model.complete_item(id, Some("/tmp/video.mkv"));
                model.item(id).expect("job").clone()
            }
            ItemStatus::Skipped => {
                model.skip_already_downloaded(id, "/tmp/video.mkv");
                model.item(id).expect("job").clone()
            }
            ItemStatus::Queued => model.item(id).expect("job").clone(),
        }
    }

    #[test]
    fn action_labels_follow_the_current_job() {
        assert_eq!(QuickAction::Start.label(), "ダウンロード");
        assert_eq!(QuickAction::Redownload.label(), "再ダウンロード");
        assert_eq!(QuickAction::Cancel.label(), "キャンセル");
        assert_eq!(QuickAction::Retry.label(), "再試行");
        assert_eq!(QuickAction::Resume.label(), "開始");
    }

    #[test]
    fn completed_urls_can_be_found_even_when_they_are_not_the_current_job() {
        let mut model = QueueModel::new();
        let first = model
            .add_single_url(
                "https://example.com/a",
                DEFAULT_QUALITY,
                DEFAULT_AUDIO,
                DEFAULT_CONTAINER,
                "/tmp",
            )
            .expect("add first URL");
        model.complete_item(first, Some("/tmp/a.mkv"));
        model
            .add_single_url(
                "https://example.com/b",
                DEFAULT_QUALITY,
                DEFAULT_AUDIO,
                DEFAULT_CONTAINER,
                "/tmp",
            )
            .expect("add second URL");

        assert_eq!(redownload_id(&model, "https://example.com/a"), Some(first));
        assert_eq!(redownload_id(&model, "https://example.com/b"), None);
    }

    #[test]
    fn download_phases_are_derived_from_progress() {
        assert_eq!(
            status_label(&job(ItemStatus::Downloading, "https://example.com/a")),
            "取得中"
        );
        assert_eq!(
            status_label(&job(ItemStatus::Failed, "https://example.com/a")),
            "失敗"
        );
        assert_eq!(
            status_label(&job(ItemStatus::Completed, "https://example.com/a")),
            "完了"
        );
        assert_eq!(
            status_label(&job(ItemStatus::Skipped, "https://example.com/a")),
            "保存済み"
        );
    }

    #[test]
    fn authentication_errors_offer_the_cookie_settings() {
        assert!(requires_authentication(
            "ERROR: Sign in to confirm you’re not a bot"
        ));
        assert!(requires_authentication("YouTubeの認証が必要です"));
        assert!(!requires_authentication(
            "HTTP Error 429: Too Many Requests"
        ));
    }
}
