//! アプリケーションのルートビュー。ツールバー、キュー、ログ、ステータスバーを組み立てる。

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::Focusable as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IndexPath, Selectable as _, Sizable as _, StyledExt as _, TitleBar,
    WindowExt as _,
    alert::Alert,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    link::Link,
    notification::Notification,
    progress::{Progress, ProgressCircle},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    separator::Separator,
    spinner::Spinner,
    status_bar::StatusBar,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_settings::{APP_DISPLAY_NAME, APP_VERSION, WallpaperSource};
use crate::downloader::DownloadManager;
use crate::model::{
    AudioQuality, Container, DEFAULT_AUDIO, DEFAULT_AUDIO_CONTAINER, DEFAULT_CONTAINER,
    DEFAULT_QUALITY, Quality, QueueEvent, QueueModel, container_index, resolve_container,
};
use crate::queue_table::render_queue;
use crate::quick_download::{QuickDownloadEvent, QuickDownloadView};
use crate::settings_page::SettingsPage;
use crate::tool_bootstrap::{BootstrapPhase, ToolBootstrap};
use crate::tools;
use crate::update_controller::{
    InstallAvailableUpdates, LibraryUpdateState, UpdateController, UpdateState,
};
use crate::wallpaper::{WallpaperState, wallpaper_layer, wallpaper_source_image};

/// 画面上部のメニューから切り替える画面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Batch,
    Quick,
    Settings,
    About,
}

gpui_kit::actions!([AddUrls, StartOrPause, RetryFailed, CheckForUpdates, QuitApp]);

pub struct AppView {
    tool_bootstrap: Entity<ToolBootstrap>,
    update_controller: Entity<UpdateController>,
    queue: Entity<QueueModel>,
    downloads: Entity<DownloadManager>,
    quick_download: Entity<QuickDownloadView>,
    wallpaper: Entity<WallpaperState>,
    settings_page: Entity<SettingsPage>,
    url_input: Entity<TextareaState>,
    add_quality: Entity<SelectState<Vec<&'static str>>>,
    add_audio: Entity<SelectState<Vec<&'static str>>>,
    add_container: Entity<SelectState<Vec<&'static str>>>,
    add_destination: Entity<SelectState<SearchableVec<SharedString>>>,
    running: bool,
    page: Page,
    wallpaper_source: Option<WallpaperSource>,
    wallpaper_image: Option<Arc<RenderImage>>,
    wallpaper_previous_image: Option<Arc<RenderImage>>,
    wallpaper_transition: u64,
    wallpaper_initialized: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tool_bootstrap = cx.new(|_| ToolBootstrap::new());
        tool_bootstrap.update(cx, |bootstrap, cx| bootstrap.start(cx));
        let update_controller = cx.new(|_| UpdateController::new());
        update_controller.update(cx, |controller, cx| controller.check_background(window, cx));
        let update_subscription = cx.observe(&update_controller, |_, _, cx| cx.notify());
        let library_update_controller = update_controller.clone();
        let bootstrap_subscription = cx.observe(&tool_bootstrap, move |_, bootstrap, cx| {
            cx.notify();
            let phase = bootstrap.read(cx).phase().clone();
            match phase {
                BootstrapPhase::Ready => {
                    library_update_controller.update(cx, |controller, cx| {
                        controller.check_libraries_background(cx);
                    });
                }
                BootstrapPhase::Failed(_) => {
                    library_update_controller.update(cx, |controller, cx| {
                        controller.mark_library_check_unavailable(cx);
                    });
                }
                _ => {}
            }
        });
        if tool_bootstrap.read(cx).is_ready() {
            update_controller.update(cx, |controller, cx| {
                controller.check_libraries_background(cx);
            });
        }
        let queue = cx.new(|_| QueueModel::new());
        let url_input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(
                "https://www.youtube.com/watch?v=...\nhttps://www.youtube.com/@channel",
            )
        });
        let add_quality = cx.new(|cx| {
            SelectState::new(
                Quality::ALL.map(Quality::label).to_vec(),
                Some(IndexPath::new(DEFAULT_QUALITY.index())),
                window,
                cx,
            )
        });
        let add_audio = cx.new(|cx| {
            SelectState::new(
                AudioQuality::ALL.map(AudioQuality::label).to_vec(),
                Some(IndexPath::new(DEFAULT_AUDIO.index())),
                window,
                cx,
            )
        });
        let wallpaper = cx.new(|_| WallpaperState {
            source: None,
            opacity: 1.0,
        });
        let settings_page =
            cx.new(|cx| SettingsPage::new(queue.clone(), wallpaper.clone(), window, cx));
        settings_page.update(cx, |page, cx| {
            page.restart_wallpaper_shuffle(window, cx);
        });
        let downloads = cx.new(|_| DownloadManager::new(queue.clone()));
        let quick_download = cx.new(|cx| {
            QuickDownloadView::new(
                queue.clone(),
                downloads.clone(),
                update_controller.clone(),
                window,
                cx,
            )
        });
        let quick_subscription = cx.subscribe(&quick_download, |this, _, event, cx| {
            match event {
                QuickDownloadEvent::OpenBatch => this.page = Page::Batch,
                QuickDownloadEvent::OpenSettings => this.page = Page::Settings,
            }
            cx.notify();
        });
        let settings_container = queue.read(cx).settings().container();
        let video_container_index =
            container_index(&Container::VIDEO, settings_container, DEFAULT_CONTAINER);
        let add_container = cx.new(|cx| {
            SelectState::new(
                Container::VIDEO.map(Container::label).to_vec(),
                Some(IndexPath::new(video_container_index)),
                window,
                cx,
            )
        });
        let destination_labels: Vec<SharedString> = queue
            .read(cx)
            .destinations()
            .iter()
            .map(SharedString::from)
            .collect();
        let add_destination = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(destination_labels),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
            .searchable(true)
        });

        let running = queue.read(cx).active_item().is_some();
        let subscription = cx.subscribe(&queue, |this, _, _: &QueueEvent, cx| {
            this.running = this.queue.read(cx).active_item().is_some();
            cx.notify();
        });

        let appearance_subscription = window.observe_window_appearance({
            let settings_page = settings_page.clone();
            move |window, cx| {
                settings_page.update(cx, |page, cx| {
                    page.apply_system_appearance(window, cx);
                });
            }
        });

        let wallpaper_subscription = cx.observe(&wallpaper, |_, _, cx| cx.notify());
        let downloads_subscription = cx.observe(&downloads, |_, _, cx| cx.notify());

        let container_subscription = cx.subscribe_in(
            &add_quality,
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
                this.sync_add_container(quality, false, window, cx);
            },
        );

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        Self {
            tool_bootstrap,
            update_controller,
            queue,
            downloads,
            quick_download,
            wallpaper,
            settings_page,
            url_input,
            add_quality,
            add_audio,
            add_container,
            add_destination,
            running,
            page: Page::Batch,
            wallpaper_source: None,
            wallpaper_image: None,
            wallpaper_previous_image: None,
            wallpaper_transition: 0,
            wallpaper_initialized: false,
            focus_handle,
            _subscriptions: vec![
                subscription,
                container_subscription,
                appearance_subscription,
                wallpaper_subscription,
                downloads_subscription,
                quick_subscription,
                bootstrap_subscription,
                update_subscription,
            ],
        }
    }

    /// メニューバー・Aboutから手動の更新確認を開始する。
    pub fn check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tool_bootstrap.read(cx).is_ready() {
            return;
        }
        self.update_controller
            .update(cx, |controller, cx| controller.check_manually(window, cx));
    }

    fn is_updating(&self, cx: &Context<Self>) -> bool {
        let controller = self.update_controller.read(cx);
        controller.is_installing() || controller.is_library_installing()
    }

    fn can_start_update(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.running {
            window.push_notification(
                Notification::warning("実行中のダウンロードを停止してから更新を開始してください。"),
                cx,
            );
            return false;
        }
        true
    }

    fn open_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller.update(cx, |controller, cx| {
            controller.open_available_dialog(window, cx)
        });
    }

    fn open_app_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller.update(cx, |controller, cx| {
            controller.open_app_update_dialog(window, cx)
        });
    }

    fn open_library_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller.update(cx, |controller, cx| {
            controller.open_library_update_dialog(window, cx)
        });
    }

    fn install_dialog_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller.update(cx, |controller, cx| {
            controller.install_dialog_updates(window, cx)
        });
    }

    fn retry_app_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller
            .update(cx, |controller, cx| controller.retry_app(window, cx));
    }

    fn retry_library_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller
            .update(cx, |controller, cx| controller.retry_libraries(window, cx));
    }

    fn retry_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start_update(window, cx) {
            return;
        }
        self.update_controller
            .update(cx, |controller, cx| controller.retry(window, cx));
    }

    fn toggle_running(&mut self, cx: &mut Context<Self>) {
        let updating = self.is_updating(cx);
        if updating {
            return;
        }
        if self.page == Page::Quick {
            self.quick_download
                .update(cx, |view, cx| view.start_or_cancel(false, cx));
        } else {
            self.downloads
                .update(cx, |manager, cx| manager.toggle_all(cx));
        }
    }

    /// 待機中・一時停止の項目をすべて開始する。
    fn start_pending(&mut self, cx: &mut Context<Self>) {
        if self.running || self.is_updating(cx) {
            return;
        }
        self.downloads
            .update(cx, |manager, cx| manager.start_pending_sites(cx));
    }

    fn retry_failed(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Quick {
            let updating = self.is_updating(cx);
            self.quick_download
                .update(cx, |view, cx| view.retry_current(updating, cx));
            return;
        }
        if self.running {
            return;
        }
        self.queue.update(cx, |model, cx| {
            if model.retry_failed() > 0 {
                cx.emit(QueueEvent::Changed);
                cx.notify();
            }
        });
    }

    fn open_add_urls_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            window.push_notification(Notification::warning("実行中はURLを追加できません"), cx);
            return;
        }
        self.url_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        let (quality, audio) = {
            let settings = self.queue.read(cx).settings();
            (settings.quality(), settings.audio())
        };
        self.add_quality.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(quality.index())), window, cx);
        });
        self.add_audio.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(audio.index())), window, cx);
        });
        self.sync_add_container(quality, true, window, cx);

        let destination_labels: Vec<SharedString> = self
            .queue
            .read(cx)
            .destinations()
            .iter()
            .map(SharedString::from)
            .collect();
        self.add_destination = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(destination_labels),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
            .searchable(true)
        });
        let model = self.queue.clone();
        let textarea = self.url_input.clone();
        let add_quality_select = self.add_quality.clone();
        let add_audio_select = self.add_audio.clone();
        let add_container_select = self.add_container.clone();
        let add_destination_select = self.add_destination.clone();
        let app_view = cx.entity();
        let focus_requested = std::cell::Cell::new(false);

        window.open_dialog(cx, move |dialog, window, dialog_cx| {
            let add_model = model.clone();
            let add_textarea = textarea.clone();
            let confirm_quality = add_quality_select.clone();
            let confirm_audio = add_audio_select.clone();
            let confirm_container = add_container_select.clone();
            let confirm_destination = add_destination_select.clone();
            let confirm_view = app_view.clone();

            if !focus_requested.replace(true) {
                textarea.focus_handle(dialog_cx).focus(window, dialog_cx);
            }

            dialog
                .title("URLを追加")
                .width(px(960.))
                .centered()
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child("URLを複数行入力すると一気にダウンロードキューに登録できます"),
                        )
                        .child(Textarea::new(&textarea).h(px(150.)))
                        .child(
                            h_flex()
                                .gap_3()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("画質"))
                                        .child(Select::new(&add_quality_select).w_full()),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("音質"))
                                        .child(Select::new(&add_audio_select).w_full()),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(div().text_xs().font_medium().child("コンテナ"))
                                        .child(Select::new(&add_container_select).w_full()),
                                ),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_medium()
                                        .child("ダウンロード先"),
                                )
                                .child(
                                    Select::new(&add_destination_select)
                                        .w_full()
                                        .icon(IconName::Search)
                                        .search_placeholder("保存先を検索")
                                        .highlight_matches(true)
                                        .accessibility_label("ダウンロード先"),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(dialog_cx.theme().muted_foreground)
                                        .child(
                                            "http:// または https:// で始まるURLを1行に1件ずつ貼り付けてください。動画・チャンネル・プレイリストに対応します。これらで始まらない行は追加しません。動画URLに list= が付いている場合は、その動画だけをダウンロードします。同じURLは重複して追加できません。",
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(dialog_cx.theme().muted_foreground)
                                        .child(
                                            "選択した画質・音質・コンテナ・ダウンロード先が、入力したすべてのURLに適用されます。画質で「映像なし」を選ぶと、コンテナは音声用（m4a・mp3・opus・flac）に切り替わります。",
                                        ),
                                ),
                        ),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-add")
                                .label("キャンセル")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-add")
                                .primary()
                                .label("キューに追加")
                                .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                    if confirm_view.read(cx).running {
                                        window.push_notification(
                                            Notification::warning("実行中はURLを追加できません"),
                                            cx,
                                        );
                                        return;
                                    }
                                    let value = add_textarea.read(cx).value();
                                    if value.trim().is_empty() {
                                        window.push_notification(
                                            Notification::warning(
                                                "http:// または https:// で始まるURLを1行に1件入力してください",
                                            ),
                                            cx,
                                        );
                                        return;
                                    }

                                    let quality = confirm_quality
                                        .read(cx)
                                        .selected_index(cx)
                                        .map(|index| Quality::from_index(index.row))
                                        .unwrap_or(DEFAULT_QUALITY);
                                    let audio = confirm_audio
                                        .read(cx)
                                        .selected_index(cx)
                                        .map(|index| AudioQuality::from_index(index.row))
                                        .unwrap_or(DEFAULT_AUDIO);
                                    let container = confirm_container
                                        .read(cx)
                                        .selected_index(cx)
                                        .and_then(|index| {
                                            Container::options_for(quality)
                                                .get(index.row)
                                                .copied()
                                        })
                                        .unwrap_or_else(|| {
                                            if quality == Quality::AudioOnly {
                                                DEFAULT_AUDIO_CONTAINER
                                            } else {
                                                DEFAULT_CONTAINER
                                            }
                                        });
                                    let destination = confirm_destination
                                        .read(cx)
                                        .selected_index(cx)
                                        .and_then(|index| {
                                            add_model
                                                .read(cx)
                                                .destinations()
                                                .get(index.row)
                                                .cloned()
                                        })
                                        .unwrap_or_default();
                                    let added = add_model.update(cx, |model, cx| {
                                        let added = model.add_urls_with(
                                            &value,
                                            quality,
                                            audio,
                                            container,
                                            &destination,
                                        );
                                        if added > 0 {
                                            cx.emit(QueueEvent::Changed);
                                            cx.notify();
                                        }
                                        added
                                    });

                                    if added == 0 {
                                        window.push_notification(
                                            Notification::warning(
                                                "追加できるURLがありません。URLの形式や重複を確認してください。",
                                            ),
                                            cx,
                                        );
                                        return;
                                    }

                                    add_textarea.update(cx, |state, cx| {
                                        state.set_value("", window, cx);
                                    });
                                    window.close_dialog(cx);
                                    window.push_notification(
                                        Notification::success(format!(
                                            "{added}件をキューに追加しました。画質などを確認して「開始する」を押してください。"
                                        )),
                                        cx,
                                    );
                                }),
                        ),
                )
        });
    }

    /// 画質に合わせて、URL追加ダイアログのコンテナ選択肢を切り替える。
    ///
    /// `reset` が false のときは、同じ種類のコンテナを選んでいればそのまま引き継ぐ。
    fn sync_add_container(
        &mut self,
        quality: Quality,
        reset: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let options = Container::options_for(quality);
        let settings_container = self.queue.read(cx).settings().container();

        let current = if reset {
            None
        } else {
            let label = self.add_container.read(cx).selected_value().copied();
            label.and_then(|label| {
                Container::ALL
                    .iter()
                    .copied()
                    .find(|container| container.label() == label)
            })
        };
        let selected_container = resolve_container(quality, current, settings_container);
        let selected = options
            .iter()
            .position(|container| *container == selected_container)
            .unwrap_or(0);
        let labels: Vec<&'static str> = options.iter().map(|container| container.label()).collect();

        self.add_container.update(cx, |state, cx| {
            state.set_items(labels, window, cx);
            state.set_selected_index(Some(IndexPath::new(selected)), window, cx);
        });
    }

    fn open_clear_queue_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let model = self.queue.clone();
        let app_view = cx.entity();

        window.open_alert_dialog(cx, move |alert, _, dialog_cx| {
            let clear_model = model.clone();
            let confirm_view = app_view.clone();

            alert
                .icon(
                    Icon::new(IconName::ListX)
                        .small()
                        .text_color(dialog_cx.theme().danger),
                )
                .title("キューを空にしますか？")
                .description(
                    "待機中・失敗・完了を含むすべての項目がキューから削除されます。ダウンロード済みのファイルは削除されません。",
                )
                .show_cancel(true)
                .centered()
                .ok_text("空にする")
                .cancel_text("キャンセル")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    if confirm_view.read(cx).running {
                        window.push_notification(
                            Notification::warning("実行中はキューを空にできません"),
                            cx,
                        );
                        return false;
                    }
                    clear_model.update(cx, |model, cx| {
                        model.clear();
                        cx.emit(QueueEvent::Changed);
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// 画面上部のページ切り替えメニュー。
    fn render_top_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;

        h_flex()
            .h_12()
            .px_4()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .child(self.nav_button(
                "nav-batch",
                Page::Batch,
                "一括ダウンロード",
                IconName::ListVideo,
                page,
                cx,
            ))
            .child(self.nav_button(
                "nav-quick",
                Page::Quick,
                "かんたんダウンロード",
                IconName::Zap,
                page,
                cx,
            ))
            .child(self.nav_button(
                "nav-settings",
                Page::Settings,
                "設定",
                IconName::Settings2,
                page,
                cx,
            ))
            .child(self.nav_button("nav-about", Page::About, "About", IconName::Info, page, cx))
    }

    fn nav_button(
        &self,
        id: &'static str,
        target: Page,
        label: &'static str,
        icon: IconName,
        current: Page,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .selected(target == current)
            .icon(Icon::new(icon).small())
            .label(label)
            .accessibility_label(if target == current {
                format!("{label}（現在の画面）")
            } else {
                label.to_string()
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_page(target, window, cx);
            }))
    }

    fn open_page(&mut self, target: Page, window: &mut Window, cx: &mut Context<Self>) {
        let changed = self.page != target;
        if changed {
            self.page = target;
            cx.notify();
        }
        if changed && target == Page::Quick {
            self.quick_download.update(cx, |view, cx| {
                view.sync_destinations(window, cx);
                view.focus_url(window, cx);
            });
        }
    }

    fn render_batch_page(
        &self,
        total: usize,
        completed: usize,
        overall: f32,
        empty: bool,
        finished: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.render_toolbar(empty, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_queue(total, completed, overall, finished, cx)),
            )
    }

    fn render_quick_page(&self, _: &mut Context<Self>) -> AnyElement {
        self.quick_download.clone().into_any_element()
    }

    fn render_about_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let yt_dlp_version =
            tools::installed_version(tools::YT_DLP).unwrap_or_else(|| "未取得".to_string());
        let ffmpeg_version =
            tools::installed_version(tools::FFMPEG).unwrap_or_else(|| "未取得".to_string());
        let deno_version =
            tools::installed_version(tools::DENO).unwrap_or_else(|| "未取得".to_string());
        let (update_state, library_state, checking, installing) = {
            let controller = self.update_controller.read(cx);
            (
                controller.state().clone(),
                controller.library_state().clone(),
                controller.is_checking() || controller.is_library_checking(),
                controller.is_installing() || controller.is_library_installing(),
            )
        };
        let update_action = match &update_state {
            UpdateState::Available(_) => Some(
                Button::new("install-app-update-about")
                    .primary()
                    .flex_1()
                    .label("インストール")
                    .disabled(checking || installing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_app_update_dialog(window, cx);
                    })),
            ),
            UpdateState::Installed { .. } => Some(
                Button::new("relaunch-app-update-about")
                    .success()
                    .flex_1()
                    .label("再起動")
                    .disabled(self.running || checking || installing)
                    .tooltip("ダウンロード中は再起動できません")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.update_controller
                            .update(cx, |controller, cx| controller.relaunch(window, cx));
                    })),
            ),
            UpdateState::Failed(_) => Some(
                Button::new("retry-app-update-about")
                    .flex_1()
                    .label("再実行")
                    .disabled(checking || installing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.retry_app_update(window, cx);
                    })),
            ),
            _ => None,
        };
        let library_update_action = match &library_state {
            LibraryUpdateState::Available(_) => Some(
                Button::new("install-library-update-about")
                    .primary()
                    .flex_1()
                    .label("インストール")
                    .disabled(checking || installing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_library_update_dialog(window, cx);
                    })),
            ),
            LibraryUpdateState::Failed(_) => Some(
                Button::new("retry-library-update-about")
                    .flex_1()
                    .label("再実行")
                    .disabled(checking || installing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.retry_library_update(window, cx);
                    })),
            ),
            _ => None,
        };
        h_flex().size_full().justify_center().child(
            div()
                .id("about-scroll")
                .w_full()
                .max_w(px(680.))
                .h_full()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .gap_6()
                        .px_6()
                        .py_8()
                        .child(
                            v_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    img("images/icon-tree-01.png")
                                        .w(px(72.))
                                        .h(px(80.))
                                        .object_fit(ObjectFit::Contain),
                                )
                                .child(div().text_xl().font_semibold().child(APP_DISPLAY_NAME)),
                        )
                        .child(Separator::horizontal())
                        .child(about_row("バージョン", APP_VERSION.to_string(), cx))
                        .child(about_row(
                            "GitHub",
                            Link::new("about-github")
                                .href("https://github.com/treetips/tree-movie-downloader")
                                .child("https://github.com/treetips/tree-movie-downloader"),
                            cx,
                        ))
                        .child(about_row("Copyright", "Copyright @2026 treetips555", cx))
                        .child(about_row("yt-dlp", yt_dlp_version, cx))
                        .child(about_row("ffmpeg", ffmpeg_version, cx))
                        .child(about_row("Deno", deno_version, cx))
                        .child(Separator::horizontal())
                        .child(
                            v_flex()
                                .gap_2()
                                .child(div().text_sm().font_medium().child("アップデート"))
                                .child(
                                    Button::new("check-update")
                                        .label("アップデートを確認")
                                        .loading(checking)
                                        .disabled(checking || installing)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.check_for_updates(window, cx);
                                        })),
                                ),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .items_start()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_2()
                                        .child(div().text_sm().font_medium().child("アプリ更新"))
                                        .children(match &update_state {
                                    UpdateState::Idle => None,
                                    UpdateState::Checking => Some(
                                        Alert::info(
                                            "about-app-update-alert",
                                            "アプリの更新を確認しています…",
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::UpToDate => Some(
                                        Alert::success(
                                            "about-app-update-alert",
                                            "アプリは最新です。",
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::CheckFailed(message) => Some(
                                        Alert::error(
                                            "about-app-update-alert",
                                            format!(
                                                "アプリの更新を確認できませんでした。{message}"
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::Available(info) => Some(
                                        Alert::warning(
                                            "about-app-update-alert",
                                            app_update_available_message(&info.version),
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::Installing(phase) => Some(
                                        Alert::info(
                                            "about-app-update-alert",
                                            format!("アプリをインストール中: {}", phase.label()),
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::Installed { .. } => Some(
                                        Alert::success(
                                            "about-app-update-alert",
                                            APP_UPDATE_COMPLETED_MESSAGE,
                                        )
                                        .w_full(),
                                    ),
                                    UpdateState::Failed(message) => Some(
                                        Alert::error(
                                            "about-app-update-alert",
                                            format!(
                                                "アプリのインストールに失敗しました。{message}"
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                }),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().text_sm().font_medium().child("ライブラリ更新"))
                                    .children(match &library_state {
                                    LibraryUpdateState::Idle => None,
                                    LibraryUpdateState::Checking => Some(
                                        Alert::info(
                                            "about-library-update-alert",
                                            "ライブラリの更新を確認しています…",
                                        )
                                        .w_full(),
                                    ),
                                    LibraryUpdateState::UpToDate => Some(
                                        Alert::success(
                                            "about-library-update-alert",
                                            "ライブラリは最新です。",
                                        )
                                        .w_full(),
                                    ),
                                    LibraryUpdateState::CheckFailed(message) => Some(
                                        Alert::error(
                                            "about-library-update-alert",
                                            format!(
                                                "ライブラリの更新を確認できませんでした。{message}"
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                    LibraryUpdateState::Available(entries) => Some(
                                        Alert::warning(
                                            "about-library-update-alert",
                                            format!(
                                                "{} をアップデートできます。",
                                                entries
                                                    .iter()
                                                    .map(|entry| format!(
                                                        "{} {}",
                                                        entry.name, entry.version
                                                    ))
                                                    .collect::<Vec<_>>()
                                                    .join("、")
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                    LibraryUpdateState::Installing(phase) => Some(
                                        Alert::info(
                                            "about-library-update-alert",
                                            format!(
                                                "ライブラリをインストール中: {}",
                                                phase.label()
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                    LibraryUpdateState::Failed(message) => Some(
                                        Alert::error(
                                            "about-library-update-alert",
                                            format!(
                                                "ライブラリのインストールに失敗しました。{message}"
                                            ),
                                        )
                                        .w_full(),
                                    ),
                                }),
                            )
                        )
                        .when(
                            update_action.is_some() || library_update_action.is_some(),
                            |this| {
                                this.child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .children(update_action)
                                        .children(library_update_action),
                                )
                            },
                        ),
                ),
        )
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .flex_1()
                .items_center()
                .gap_2()
                .px_3()
                .child(
                    Icon::new(IconName::Clapperboard)
                        .small()
                        .text_color(cx.theme().primary),
                )
                .child(div().text_sm().font_semibold().child(APP_DISPLAY_NAME))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("v{}", APP_VERSION)),
                ),
        )
    }

    /// ヘッダー下の右上に重ねる更新通知。
    fn render_update_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let (
            state,
            library_state,
            available_label,
            no_update_visible,
            app_visible,
            library_visible,
        ) = {
            let controller = self.update_controller.read(cx);
            (
                controller.state().clone(),
                controller.library_state().clone(),
                controller.available_update_label(),
                controller.no_update_visible(),
                controller.is_app_update_notification_visible(),
                controller.is_library_update_notification_visible(),
            )
        };
        let state = if app_visible {
            state
        } else {
            UpdateState::Idle
        };
        let library_state = if library_visible {
            library_state
        } else {
            LibraryUpdateState::Idle
        };

        let (tint, opacity, content): (Hsla, f32, AnyElement) = if state.is_checking()
            || library_state.is_checking()
        {
            let color = cx.theme().muted_foreground;
            (
                cx.theme().muted,
                0.12,
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        ProgressCircle::new("update-checking")
                            .loading(true)
                            .size_5()
                            .color(color)
                            .accessibility_label("アップデートを確認中"),
                    )
                    .child(div().text_sm().font_medium().child("アップデートを確認中"))
                    .into_any_element(),
            )
        } else if let Some((label, percent, color)) = match (&state, &library_state) {
            (UpdateState::Installing(phase), _) => Some((
                phase.status_label().to_string(),
                phase.percent(),
                cx.theme().blue,
            )),
            (_, LibraryUpdateState::Installing(phase)) => {
                Some((phase.status_label(), phase.percent(), cx.theme().primary))
            }
            _ => None,
        } {
            let mut row = h_flex()
                .items_center()
                .gap_2()
                .child(
                    ProgressCircle::new("update-install-progress")
                        .loading(percent.is_none())
                        .value(percent.unwrap_or(0.0))
                        .size_5()
                        .color(color)
                        .accessibility_label("アップデートのインストール進捗"),
                )
                .child(div().text_sm().font_medium().child(label));
            if let Some(percent) = percent {
                row = row.child(
                    div()
                        .min_w_10()
                        .text_right()
                        .text_sm()
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(format!("{percent:.0}%")),
                );
            }
            (color, 0.12, row.into_any_element())
        } else if matches!(&state, UpdateState::Failed(_))
            || matches!(&library_state, LibraryUpdateState::Failed(_))
        {
            let color = cx.theme().danger;
            (
                color,
                0.12,
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Self::update_status_icon(IconName::CircleAlert, color))
                    .child(div().text_sm().font_medium().child("インストールに失敗"))
                    .child(
                        Self::update_action_button(
                            "retry-update-title",
                            "再実行",
                            ButtonVariant::Danger,
                        )
                        .disabled(self.running)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.retry_update(window, cx);
                        })),
                    )
                    .child(self.update_close_button(cx))
                    .into_any_element(),
            )
        } else if let UpdateState::Installed { .. } = &state {
            let color = cx.theme().success;
            (
                color,
                0.12,
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Self::update_status_icon(IconName::CircleCheck, color))
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child("アップデートが完了しました"),
                    )
                    .child(
                        Self::update_action_button(
                            "relaunch-update-title",
                            "再起動",
                            ButtonVariant::Success,
                        )
                        .disabled(self.running)
                        .tooltip("ダウンロード中は再起動できません")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.update_controller
                                .update(cx, |controller, cx| controller.relaunch(window, cx));
                        })),
                    )
                    .child(self.update_close_button(cx))
                    .into_any_element(),
            )
        } else if let Some(label) = available_label {
            let color = cx.theme().blue;
            (
                color,
                0.12,
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Self::update_status_icon(IconName::Download, color))
                    .child(div().text_sm().font_medium().child(label))
                    .child(
                        Self::update_action_button(
                            "update-area-install",
                            "インストール",
                            ButtonVariant::Primary,
                        )
                        .disabled(self.running)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_update_dialog(window, cx);
                        })),
                    )
                    .child(self.update_close_button(cx))
                    .into_any_element(),
            )
        } else if no_update_visible {
            (
                cx.theme().muted,
                0.05,
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Self::update_status_icon(
                        IconName::CircleCheck,
                        cx.theme().success,
                    ))
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child("アップデートは有りません"),
                    )
                    .child(self.update_close_button(cx))
                    .into_any_element(),
            )
        } else {
            return div().into_any_element();
        };

        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .min_h_9()
            .rounded_full()
            .border_1()
            .border_color(tint.opacity(0.35))
            .bg(tint.opacity(opacity))
            .text_color(cx.theme().foreground)
            .shadow_sm()
            .child(content)
            .into_any_element()
    }

    fn update_close_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("dismiss-update-notification")
            .small()
            .icon(IconName::Close)
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(cx.theme().muted.opacity(0.6))
                    .foreground(cx.theme().foreground)
                    .hover(cx.theme().muted.opacity(0.25))
                    .active(cx.theme().muted.opacity(0.4)),
            )
            .rounded_full()
            .p_0()
            .tooltip("閉じる")
            .accessibility_label("閉じる")
            .on_click(cx.listener(|this, _, _, cx| {
                this.update_controller
                    .update(cx, |controller, cx| controller.dismiss_notification(cx));
            }))
    }

    fn update_status_icon(icon: IconName, color: Hsla) -> impl IntoElement {
        h_flex()
            .size_7()
            .flex_shrink_0()
            .justify_center()
            .rounded_full()
            .bg(color.opacity(0.18))
            .child(Icon::new(icon).text_color(color))
    }

    fn update_action_button(
        id: &'static str,
        label: &'static str,
        variant: ButtonVariant,
    ) -> Button {
        Button::new(id)
            .small()
            .with_variant(variant)
            .label(label)
            .font_semibold()
            .rounded_full()
            .shadow_none()
    }

    /// 初回起動時など、ツールが揃うまでのダウンロード画面。
    fn render_tool_bootstrap(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let phase = self.tool_bootstrap.read(cx).phase().clone();
        let failed = matches!(phase, BootstrapPhase::Failed(_));

        let detail: AnyElement = match &phase {
            BootstrapPhase::Checking => h_flex()
                .items_center()
                .gap_2()
                .child(Spinner::new().small().color(cx.theme().primary))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("配布情報を確認しています…"),
                )
                .into_any_element(),
            BootstrapPhase::Downloading {
                tool,
                received,
                total,
                step,
                steps,
            } => {
                let percent = total
                    .filter(|total| *total > 0)
                    .map(|total| (*received as f32 / total as f32) * 100.0)
                    .unwrap_or(0.0);
                let amount = match total.filter(|total| *total > 0) {
                    Some(total) => format!("{} / {}", format_bytes(*received), format_bytes(total)),
                    None => format_bytes(*received),
                };
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(format!("{tool} をダウンロードしています")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{step} / {steps}")),
                            ),
                    )
                    .child(
                        Progress::new(("tools-download", *step))
                            .value(percent)
                            .loading(total.is_none())
                            .accessibility_label("ツールのダウンロード進捗"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(amount),
                    )
                    .into_any_element()
            }
            BootstrapPhase::Installing { tool, step, steps } => v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(format!("{tool} を配置しています…")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{step} / {steps}")),
                        ),
                )
                .child(
                    Progress::new(("tools-install", *step))
                        .loading(true)
                        .accessibility_label("ツールの配置進捗"),
                )
                .into_any_element(),
            BootstrapPhase::Failed(message) => v_flex()
                .gap_2()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().danger.opacity(0.35))
                .bg(cx.theme().danger.opacity(0.08))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Icon::new(IconName::CircleAlert)
                                .small()
                                .text_color(cx.theme().danger),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .text_color(cx.theme().danger)
                                .child("準備に失敗しました"),
                        ),
                )
                .child(div().text_sm().child(message.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("ネットワーク接続を確認して、再試行してください。"),
                )
                .into_any_element(),
            BootstrapPhase::Ready => h_flex().into_any_element(),
        };

        let retry = failed.then(|| {
            Button::new("retry-tools")
                .primary()
                .label("再試行")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tool_bootstrap.update(cx, |bootstrap, cx| {
                        bootstrap.retry(cx);
                    });
                }))
        });

        h_flex().size_full().justify_center().items_center().p_8().child(
            v_flex()
                .w(px(560.))
                .max_w_full()
                .gap_5()
                .p_8()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size_10()
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().primary.opacity(0.12))
                                .child(
                                    Icon::new(IconName::Download)
                                        .small()
                                        .text_color(cx.theme().primary),
                                ),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_semibold()
                                        .child("動画ツールを準備しています"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "yt-dlp・ffmpeg・Deno のうち、未準備・更新対象のツールを GitHub Releases から取得します。日常の更新確認とは分けて扱い、更新の自動インストールはしません。",
                                        ),
                                ),
                        ),
                )
                .child(detail)
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("準備が完了すると自動的にメイン画面へ進みます。"),
                        )
                        .children(retry),
                ),
        )
    }

    fn can_add_urls(running: bool) -> bool {
        !running
    }

    fn can_clear_queue(running: bool, empty: bool) -> bool {
        !running && !empty
    }

    fn can_start_queue(running: bool, has_pending: bool) -> bool {
        !running && has_pending
    }

    fn render_toolbar(&self, empty: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.running;
        let updating = self.is_updating(cx);
        let can_clear = Self::can_clear_queue(running, empty);
        let can_start =
            Self::can_start_queue(running, self.queue.read(cx).has_pending()) && !updating;

        h_flex()
            .h_12()
            .px_4()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Self::add_urls_button("add-urls", running, cx))
            .child(
                Button::new("start-queue")
                    .icon(IconName::Play)
                    .label("開始する")
                    .tooltip(if updating {
                        "更新中は開始できません"
                    } else if running {
                        "実行中は開始できません"
                    } else {
                        "待機中のキューを開始"
                    })
                    .disabled(!can_start)
                    .when(can_start, |this| this.primary().large().font_semibold())
                    .on_click(cx.listener(|this, _, _, cx| this.start_pending(cx))),
            )
            .child(
                Button::new("clear-queue")
                    .icon(IconName::ListX)
                    .label("キューを空にする")
                    .tooltip(if running {
                        "実行中はキューを空にできません"
                    } else {
                        "キューを空にする"
                    })
                    .disabled(!can_clear)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_clear_queue_dialog(window, cx)),
                    ),
            )
    }

    fn add_urls_button(id: &'static str, running: bool, cx: &mut Context<Self>) -> Button {
        let can_add = Self::can_add_urls(running);
        let text_color = cx.theme().yellow;

        Button::new(id)
            .icon(IconName::Plus)
            .label("URLを追加…")
            .tooltip(if running {
                "実行中はURLを追加できません"
            } else {
                "URLを追加（⌘N）"
            })
            .disabled(!can_add)
            .when(can_add, |this| {
                this.secondary().large().font_bold().text_color(text_color)
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_add_urls_dialog(window, cx)))
    }

    fn render_queue(
        &self,
        total: usize,
        completed: usize,
        overall: f32,
        finished: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let content: AnyElement = if total == 0 {
            render_empty_queue(self.running, cx).into_any_element()
        } else {
            render_queue(
                &self.queue,
                &self.downloads,
                self.running,
                self.is_updating(cx),
                cx,
            )
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h_11()
                    .px_4()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_1()
                            .child(
                                Icon::new(IconName::ListVideo)
                                    .xsmall()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .child(format!("{total}件のキュー")),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{completed}/{total} 完了")),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            gpui_kit::component::progress::Progress::new("queue-overall")
                                .value(overall)
                                .small(),
                        ),
                    )
                    .child(
                        div()
                            .w_10()
                            .text_xs()
                            .text_right()
                            .text_color(if finished {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(if finished {
                                "終了".to_string()
                            } else {
                                format!("{overall:.0}%")
                            }),
                    ),
            )
            .child(
                div()
                    .id("queue-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .child(content),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_status_bar(
        &self,
        running: bool,
        total: usize,
        completed: usize,
        failed: usize,
        active_count: usize,
        active_url: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (status_color, status_label) = if running {
            (cx.theme().success, "実行中")
        } else {
            (cx.theme().muted_foreground, "待機中")
        };

        let center = match active_url {
            Some(url) if active_count > 1 => div()
                .max_w(px(560.))
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{active_count}サイトで取得中: {url}"))
                .into_any_element(),
            Some(url) => div()
                .max_w(px(560.))
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(format!("取得中: {url}"))
                .into_any_element(),
            None if total == 0 => div()
                .text_color(cx.theme().muted_foreground)
                .child("キューは空です")
                .into_any_element(),
            None => div()
                .text_color(cx.theme().muted_foreground)
                .child("待機中")
                .into_any_element(),
        };

        StatusBar::new()
            .left(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(div().size_2().rounded_full().bg(status_color))
                    .child(div().font_medium().child(status_label)),
            )
            .left(Separator::vertical())
            .left(format!("完了 {completed} / {total}"))
            .left(Separator::vertical())
            .left(format!("失敗 {failed}"))
            .child(center)
    }
}

fn render_empty_queue(running: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .text_center()
        .child(
            Icon::new(IconName::ListVideo)
                .size_10()
                .text_color(cx.theme().muted_foreground),
        )
        .child(div().text_sm().font_medium().child("キューは空です"))
        .child(AppView::add_urls_button("empty-add-urls", running, cx))
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = value as f64;
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

pub(crate) const APP_UPDATE_COMPLETED_MESSAGE: &str =
    "アプリの更新が完了しました。再起動すると適用されます。";

pub(crate) fn app_update_available_message(version: &str) -> String {
    format!("アプリを v{version} にアップデートできます。")
}

fn about_row(label: &'static str, value: impl IntoElement, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .gap_4()
        .child(
            div()
                .w_24()
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().text_sm().child(value))
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (total, completed, failed, overall, active_count, active_url, finished) = {
            let model = self.queue.read(cx);
            let active = model.active_item();
            (
                model.items().len(),
                model.completed_count(),
                model.failed_count(),
                model.overall_progress(),
                model.active_count(),
                active.map(|item| item.url().clone()),
                model.is_finished(),
            )
        };
        let running = self.running;
        let empty = total == 0;
        let page = self.page;
        let tools_ready = self.tool_bootstrap.read(cx).is_ready();

        let page_content: AnyElement = match page {
            Page::Batch => self
                .render_batch_page(total, completed, overall, empty, finished, cx)
                .into_any_element(),
            Page::Quick => self.render_quick_page(cx),
            Page::Settings => self.settings_page.clone().into_any_element(),
            Page::About => self.render_about_page(cx).into_any_element(),
        };

        let wallpaper = self.wallpaper.read(cx).clone();
        // ツール準備中は背景画像のデコードを後回しにし、ダウンロード開始を優先する。
        let source_changed = tools_ready && self.wallpaper_source != wallpaper.source;
        let start_fade = self.wallpaper_initialized && source_changed;
        if source_changed {
            self.wallpaper_previous_image = self.wallpaper_image.take();
            self.wallpaper_image = wallpaper
                .source
                .as_ref()
                .and_then(|source| wallpaper_source_image(cx, source));
            self.wallpaper_source = wallpaper.source.clone();
            if start_fade {
                self.wallpaper_transition += 1;
            }
        }
        self.wallpaper_initialized = true;

        let animate = self.wallpaper_transition > 0;
        let fade = |el: Div, name: &'static str, direction: f32| {
            let animation =
                Animation::new(Duration::from_millis(400)).with_easing(|t| t * t * (3.0 - 2.0 * t));
            el.with_animation(
                ElementId::NamedInteger(name.into(), self.wallpaper_transition),
                animation,
                move |el, delta| {
                    let opacity = if direction > 0.0 { delta } else { 1.0 - delta };
                    el.opacity(opacity)
                },
            )
            .into_any_element()
        };

        let previous = self.wallpaper_previous_image.clone().map(|image| {
            let layer = wallpaper_layer(image, ObjectFit::Contain);
            if animate {
                fade(layer, "wallpaper-fade-out", -1.0)
            } else {
                layer.into_any_element()
            }
        });
        let current = self.wallpaper_image.clone().map(|image| {
            let layer = wallpaper_layer(image, ObjectFit::Contain);
            if animate {
                fade(layer, "wallpaper-fade-in", 1.0)
            } else {
                layer.into_any_element()
            }
        });
        let wallpaper_layers = if tools_ready && (previous.is_some() || current.is_some()) {
            Some(
                div()
                    .size_full()
                    .when_some(previous, |this, layer| {
                        this.child(div().absolute().inset_0().child(layer))
                    })
                    .when_some(current, |this, layer| {
                        this.child(div().absolute().inset_0().child(layer))
                    })
                    .into_any_element(),
            )
        } else {
            None
        };

        v_flex()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus_handle)
            .key_context("tree-movie-downloader")
            .on_action(cx.listener(|this, _: &AddUrls, window, cx| {
                if !this.tool_bootstrap.read(cx).is_ready() {
                    return;
                }
                if !window.has_active_dialog(cx) {
                    if this.page == Page::Quick {
                        this.quick_download
                            .update(cx, |view, cx| view.focus_url(window, cx));
                    } else {
                        if this.page != Page::Batch {
                            this.page = Page::Batch;
                            cx.notify();
                        }
                        this.open_add_urls_dialog(window, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &StartOrPause, window, cx| {
                if !this.tool_bootstrap.read(cx).is_ready() || window.has_active_dialog(cx) {
                    return;
                }
                this.toggle_running(cx);
            }))
            .on_action(cx.listener(|this, _: &RetryFailed, window, cx| {
                if !this.tool_bootstrap.read(cx).is_ready() || window.has_active_dialog(cx) {
                    return;
                }
                this.retry_failed(cx);
            }))
            .on_action(
                cx.listener(|this, _: &InstallAvailableUpdates, window, cx| {
                    this.install_dialog_updates(window, cx);
                }),
            )
            .child(self.render_title_bar(cx))
            .when(tools_ready, |this| this.child(self.render_top_menu(cx)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .when_some(wallpaper_layers, |this, layers| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .overflow_hidden()
                                .opacity(wallpaper.opacity.clamp(0.0, 1.0))
                                .child(layers),
                        )
                    })
                    .child(div().size_full().child(if tools_ready {
                        page_content
                    } else {
                        self.render_tool_bootstrap(cx).into_any_element()
                    }))
                    .child(
                        h_flex()
                            .absolute()
                            .top_0()
                            .right_3()
                            .h_12()
                            .items_center()
                            .child(self.render_update_area(cx)),
                    ),
            )
            .when(tools_ready, |this| {
                this.child(self.render_status_bar(
                    running,
                    total,
                    completed,
                    failed,
                    active_count,
                    active_url,
                    cx,
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::AppView;

    #[test]
    fn toolbar_actions_respect_running_and_queue_state() {
        assert!(AppView::can_add_urls(false));
        assert!(!AppView::can_add_urls(true));

        assert!(AppView::can_clear_queue(false, false));
        assert!(!AppView::can_clear_queue(false, true));
        assert!(!AppView::can_clear_queue(true, false));
        assert!(!AppView::can_clear_queue(true, true));

        assert!(AppView::can_start_queue(false, true));
        assert!(!AppView::can_start_queue(true, true));
        assert!(!AppView::can_start_queue(false, false));
        assert!(!AppView::can_start_queue(true, false));
    }
}
