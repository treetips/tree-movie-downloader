//! サイトごとに分かれたダウンロードキュー。

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    progress::Progress,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::activity_log::{logs_as_text, render_site_logs};
use crate::downloader::DownloadManager;
use crate::file_actions::open_output_file;
use crate::model::{
    AudioQuality, Container, DEFAULT_QUALITY, ItemStatus, Quality, QueueEvent, QueueItem,
    QueueModel, Site, compact_url, display_url,
};

const W_STATUS: f32 = 72.;
const W_URL: f32 = 188.;
const W_QUALITY: f32 = 92.;
const W_AUDIO: f32 = 88.;
const W_CONTAINER: f32 = 92.;
const W_DESTINATION: f32 = 132.;
const W_PROGRESS: f32 = 88.;
/// 「444.9 KB」と「/ 444.9 KB」を2行で折り返さずに表示できる幅。
const W_SIZE: f32 = 76.;
const W_ACTIONS: f32 = 212.;
const TABLE_WIDTH: f32 = W_STATUS
    + W_URL
    + W_QUALITY
    + W_AUDIO
    + W_CONTAINER
    + W_DESTINATION
    + W_PROGRESS
    + W_SIZE
    + W_ACTIONS;

#[derive(Clone, Copy)]
struct SiteControls {
    running: bool,
    updating: bool,
}

/// サイトごとのキュー表を組み立てる。
pub fn render_queue(
    model: &Entity<QueueModel>,
    downloads: &Entity<DownloadManager>,
    running: bool,
    updating: bool,
    cx: &mut App,
) -> AnyElement {
    let (items, destinations) = {
        let model = model.read(cx);
        (model.items().to_vec(), model.destinations().to_vec())
    };

    let mut sections = Vec::new();
    for site in Site::ALL {
        let site_items: Vec<QueueItem> = items
            .iter()
            .filter(|item| item.site() == site)
            .cloned()
            .collect();
        if site_items.is_empty() {
            continue;
        }
        sections.push(site_section(
            model.clone(),
            downloads.clone(),
            SiteControls { running, updating },
            site,
            site_items,
            &destinations,
            cx,
        ));
    }

    v_flex()
        .w_full()
        .min_w(px(TABLE_WIDTH))
        .gap_4()
        .children(sections)
        .into_any_element()
}

fn can_start_site(site_running: bool, has_startable: bool, updating: bool) -> bool {
    !updating && (site_running || has_startable)
}

fn site_section(
    model: Entity<QueueModel>,
    downloads: Entity<DownloadManager>,
    controls: SiteControls,
    site: Site,
    items: Vec<QueueItem>,
    destinations: &[String],
    cx: &mut App,
) -> AnyElement {
    let SiteControls { running, updating } = controls;
    let active = items
        .iter()
        .filter(|item| item.status() == ItemStatus::Downloading)
        .count();
    let site_running = active > 0;
    let has_startable = items
        .iter()
        .any(|item| matches!(item.status(), ItemStatus::Paused | ItemStatus::Queued));
    let can_start = can_start_site(site_running, has_startable, updating);
    let has_failed = items.iter().any(|item| item.status() == ItemStatus::Failed);
    let logs = model.read(cx).logs_for_site(site);
    let log_count = logs.len();
    let log_expanded = downloads.read(cx).log_expanded(site);
    let log_toggle_downloads = downloads.clone();
    let log_clear_model = model.clone();
    let log_menu_model = model.clone();
    let toggle_downloads = downloads.clone();
    let retry_model = model.clone();
    let clear_model = model.clone();
    let clear_downloads = downloads.clone();

    let header = h_flex()
        .h(px(38.))
        .px_3()
        .gap_2()
        .items_center()
        .bg(cx.theme().table_head)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(site.label()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{}件", items.len())),
        )
        .when(active > 0, |this| {
            this.child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(cx.theme().success)
                    .child(format!("{active}件 取得中")),
            )
        })
        .child(div().flex_1())
        .child(
            Button::new(SharedString::from(format!("site-toggle-{}", site.label())))
                .ghost()
                .xsmall()
                .icon(if site_running {
                    Icon::new(IconName::Pause)
                } else {
                    Icon::new(IconName::Play).text_color(if can_start {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    })
                })
                .label(if site_running {
                    "一時停止"
                } else {
                    "開始"
                })
                .tooltip(if updating {
                    "更新中は開始・一時停止できません"
                } else if site_running {
                    "このサイトのダウンロードを一時停止"
                } else {
                    "このサイトのダウンロードを開始"
                })
                .disabled(!can_start)
                .on_click(move |_, _, cx| {
                    toggle_downloads.update(cx, |manager, cx| {
                        manager.toggle_site(site, cx);
                    });
                }),
        )
        .child(
            Button::new(SharedString::from(format!("site-retry-{}", site.label())))
                .ghost()
                .xsmall()
                .icon(IconName::RotateCcw)
                .label("再試行")
                .tooltip(if running {
                    "実行中は再試行できません"
                } else {
                    "このサイトの失敗した項目を再試行"
                })
                .disabled(!has_failed || running)
                .on_click(move |_, _, cx| {
                    retry_model.update(cx, |model, cx| {
                        model.retry_failed_for_site(site);
                        cx.emit(QueueEvent::Changed);
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new(SharedString::from(format!("site-clear-{}", site.label())))
                .ghost()
                .xsmall()
                .icon(IconName::ListX)
                .label("空にする")
                .tooltip("このサイトのキューを空にする")
                .on_click(move |_, window, cx| {
                    open_clear_site_dialog(
                        clear_model.clone(),
                        clear_downloads.clone(),
                        site,
                        window,
                        cx,
                    );
                }),
        );

    let columns = h_flex()
        .h(px(32.))
        .w_full()
        .pl(px(3.))
        .bg(cx.theme().table_head)
        .border_b_1()
        .border_color(cx.theme().border)
        .children([
            header_cell("状態", W_STATUS, cx),
            flexible_header("URL", W_URL, cx),
            quality_header(model.clone(), site, cx),
            audio_header(model.clone(), site, cx),
            container_header(model.clone(), site, cx),
            destination_header(model.clone(), site, destinations, cx),
            header_cell("進捗", W_PROGRESS, cx),
            header_cell("サイズ", W_SIZE, cx),
            header_cell("操作", W_ACTIONS, cx),
        ]);

    let rows: Vec<AnyElement> = items
        .into_iter()
        .map(|item| queue_row(model.clone(), item, destinations, running, cx))
        .collect();

    let log_panel = v_flex()
        .id(SharedString::from(format!(
            "site-log-panel-{}",
            site.label()
        )))
        .w_full()
        .child(
            h_flex()
                .id(SharedString::from(format!(
                    "site-log-toggle-{}",
                    site.label()
                )))
                .w_full()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().list_hover))
                .on_click(move |_, _, cx| {
                    log_toggle_downloads.update(cx, |manager, cx| {
                        manager.toggle_log(site, cx);
                    });
                })
                .child(
                    Icon::new(IconName::ScrollText)
                        .xsmall()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(div().text_xs().font_medium().child("ログ"))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{log_count}件")),
                )
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!(
                        "site-log-clear-{}",
                        site.label()
                    )))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Eraser)
                    .tooltip("このサイトのログをクリア")
                    .accessibility_label("このサイトのログをクリア")
                    .disabled(log_count == 0)
                    .on_click(move |_, _, cx| {
                        log_clear_model.update(cx, |model, cx| {
                            model.clear_logs_for_site(site);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                    }),
                )
                .child(
                    Icon::new(if log_expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
                ),
        )
        .when(log_expanded, |this| {
            this.child(
                div()
                    .id(SharedString::from(format!(
                        "site-log-scroll-{}",
                        site.label()
                    )))
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(render_site_logs(&logs, cx)),
            )
        })
        .context_menu(move |mut menu, _, _| {
            let copy_model = log_menu_model.clone();
            let clear_model = log_menu_model.clone();
            menu = menu.item(
                PopupMenuItem::new("ログをコピー")
                    .icon(IconName::Copy)
                    .disabled(log_count == 0)
                    .on_click(move |_, window, cx| {
                        let logs = copy_model.read(cx).logs_for_site(site);
                        let text = logs_as_text(&logs);
                        if text.is_empty() {
                            return;
                        }
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        window.push_notification(
                            Notification::success("ログをクリップボードにコピーしました"),
                            cx,
                        );
                    }),
            );
            menu.item(
                PopupMenuItem::new("ログをクリア")
                    .icon(IconName::Eraser)
                    .disabled(log_count == 0)
                    .on_click(move |_, _, cx| {
                        clear_model.update(cx, |model, cx| {
                            model.clear_logs_for_site(site);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                    }),
            )
        });

    v_flex()
        .w_full()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_md()
        .overflow_hidden()
        .child(header)
        .child(columns)
        .children(rows)
        .child(log_panel)
        .into_any_element()
}

fn open_clear_site_dialog(
    model: Entity<QueueModel>,
    downloads: Entity<DownloadManager>,
    site: Site,
    window: &mut Window,
    cx: &mut App,
) {
    let label = site.label();
    window.open_alert_dialog(cx, move |alert, _, dialog_cx| {
        let clear_model = model.clone();
        let clear_downloads = downloads.clone();
        alert
            .icon(
                Icon::new(IconName::ListX)
                    .small()
                    .text_color(dialog_cx.theme().danger),
            )
            .title(format!("{label}のキューを空にしますか？"))
            .description(format!(
                "{label}の待機中・失敗・完了を含むすべての項目がキューから削除されます。ダウンロード済みのファイルは削除されません。"
            ))
            .show_cancel(true)
            .centered()
            .ok_text("空にする")
            .cancel_text("キャンセル")
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, _, cx| {
                clear_downloads.update(cx, |manager, _| manager.cancel_site(site));
                clear_model.update(cx, |model, cx| {
                    model.clear_site(site);
                    cx.emit(QueueEvent::Changed);
                    cx.notify();
                });
                true
            })
    });
}

fn queue_row(
    model: Entity<QueueModel>,
    item: QueueItem,
    destinations: &[String],
    running: bool,
    cx: &mut App,
) -> AnyElement {
    let border = status_border_color(item.status(), cx);
    let access = settings_access(item.status());
    let row_model = model.clone();

    h_flex()
        .w_full()
        .h(px(54.))
        .items_center()
        .border_l(px(3.))
        .border_color(border)
        .hover(|this| this.bg(cx.theme().list_hover))
        .child(cell(status_cell(&item, cx), W_STATUS))
        .child(flexible_cell(url_cell(&item, cx), W_URL))
        .child(cell(
            quality_menu(row_model.clone(), &item, access, cx),
            W_QUALITY,
        ))
        .child(cell(
            audio_menu(row_model.clone(), &item, access, cx),
            W_AUDIO,
        ))
        .child(cell(
            container_menu(row_model.clone(), &item, access, cx),
            W_CONTAINER,
        ))
        .child(cell(
            destination_menu(row_model.clone(), &item, destinations, access, cx),
            W_DESTINATION,
        ))
        .child(cell(progress_cell(&item, cx), W_PROGRESS))
        .child(cell(size_cell(&item, cx), W_SIZE))
        .child(cell(actions_cell(row_model, &item, running), W_ACTIONS))
        .into_any_element()
}

fn cell(content: impl IntoElement, width: f32) -> AnyElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .px_1()
        .min_w_0()
        .child(content)
        .into_any_element()
}

/// 余った横幅を吸収するセル。狭いときは最小幅を維持する。
fn flexible_cell(content: impl IntoElement, min_width: f32) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(min_width))
        .px_1()
        .min_w_0()
        .child(content)
        .into_any_element()
}

fn flexible_header(label: &'static str, min_width: f32, cx: &App) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(min_width))
        .px_1()
        .text_xs()
        .font_medium()
        .text_color(cx.theme().foreground)
        .child(label)
        .into_any_element()
}

fn header_cell(label: &'static str, width: f32, cx: &App) -> AnyElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .px_1()
        .text_xs()
        .font_medium()
        .text_color(cx.theme().foreground)
        .child(label)
        .into_any_element()
}

fn bulk_header(
    label: &'static str,
    width: f32,
    id: &'static str,
    enabled: bool,
    menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w(px(width))
        .flex_shrink_0()
        .px_1()
        .justify_between()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(label),
        )
        .child(
            Button::new(id)
                .ghost()
                .xsmall()
                .label("一括")
                .icon(IconName::ChevronDown)
                .tooltip("このサイトの待機中の行を一括で変更")
                .accessibility_label("このサイトの待機中の行を一括で変更")
                .disabled(!enabled)
                .dropdown_menu(menu),
        )
        .into_any_element()
}

fn quality_header(model: Entity<QueueModel>, site: Site, cx: &App) -> AnyElement {
    let current = model
        .read(cx)
        .items()
        .iter()
        .find(|item| item.site() == site && item.status() == ItemStatus::Queued)
        .map(|item| item.quality());
    let enabled = current.is_some();
    let menu_model = model.clone();
    bulk_header(
        "画質",
        W_QUALITY,
        "quality-bulk",
        enabled,
        move |mut menu, _, _| {
            for quality in Quality::ALL {
                let model = menu_model.clone();
                menu = menu.item(
                    PopupMenuItem::new(quality.label())
                        .checked(current == Some(quality))
                        .on_click(move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.set_all_quality_for_site(site, quality);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        },
        cx,
    )
}

fn audio_header(model: Entity<QueueModel>, site: Site, cx: &App) -> AnyElement {
    let current = model
        .read(cx)
        .items()
        .iter()
        .find(|item| item.site() == site && item.status() == ItemStatus::Queued)
        .map(|item| item.audio());
    let enabled = current.is_some();
    let menu_model = model.clone();
    bulk_header(
        "音質",
        W_AUDIO,
        "audio-bulk",
        enabled,
        move |mut menu, _, _| {
            for audio in AudioQuality::ALL {
                let model = menu_model.clone();
                menu = menu.item(
                    PopupMenuItem::new(audio.label())
                        .checked(current == Some(audio))
                        .on_click(move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.set_all_audio_for_site(site, audio);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        },
        cx,
    )
}

fn container_header(model: Entity<QueueModel>, site: Site, cx: &App) -> AnyElement {
    let (current, quality) = model
        .read(cx)
        .items()
        .iter()
        .find(|item| item.site() == site && item.status() == ItemStatus::Queued)
        .map(|item| (Some(item.container()), item.quality()))
        .unwrap_or((None, DEFAULT_QUALITY));
    let enabled = current.is_some();
    let menu_model = model.clone();
    bulk_header(
        "コンテナ",
        W_CONTAINER,
        "container-bulk",
        enabled,
        move |mut menu, _, _| {
            for container in Container::options_for(quality) {
                let container = *container;
                let model = menu_model.clone();
                menu = menu.item(
                    PopupMenuItem::new(container.label())
                        .checked(current == Some(container))
                        .on_click(move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.set_all_container_for_site(site, container);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        },
        cx,
    )
}

fn destination_header(
    model: Entity<QueueModel>,
    site: Site,
    destinations: &[String],
    cx: &App,
) -> AnyElement {
    let current = model
        .read(cx)
        .items()
        .iter()
        .find(|item| item.site() == site && item.status() == ItemStatus::Queued)
        .map(|item| item.destination().to_string());
    let enabled = current.is_some();
    let menu_model = model.clone();
    let destinations = destinations.to_vec();
    bulk_header(
        "DLフォルダ",
        W_DESTINATION,
        "destination-bulk",
        enabled,
        move |mut menu, _, _| {
            for destination in &destinations {
                let model = menu_model.clone();
                let value = destination.clone();
                let checked = current.as_deref() == Some(destination.as_str());
                menu = menu.item(
                    PopupMenuItem::new(destination.clone())
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.set_all_destination_for_site(site, &value);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        },
        cx,
    )
}

/// 行の設定を変更できるかどうか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsAccess {
    /// 待機中で、設定を変更できる。
    Editable,
    /// ダウンロード中で、設定を変更できない。
    Disabled,
    /// 開始済みで、設定は表示のみ。
    ReadOnly,
}

fn settings_access(status: ItemStatus) -> SettingsAccess {
    match status {
        ItemStatus::Queued => SettingsAccess::Editable,
        ItemStatus::Downloading => SettingsAccess::Disabled,
        ItemStatus::Paused | ItemStatus::Completed | ItemStatus::Failed | ItemStatus::Skipped => {
            SettingsAccess::ReadOnly
        }
    }
}

fn status_border_color(status: ItemStatus, cx: &App) -> Hsla {
    match status {
        ItemStatus::Queued => cx.theme().muted_foreground.opacity(0.35),
        ItemStatus::Downloading => cx.theme().primary,
        ItemStatus::Paused => cx.theme().warning,
        ItemStatus::Completed => cx.theme().success,
        ItemStatus::Failed => cx.theme().danger,
        ItemStatus::Skipped => cx.theme().muted_foreground.opacity(0.35),
    }
}

fn status_cell(item: &QueueItem, cx: &App) -> AnyElement {
    let (icon, color) = match item.status() {
        ItemStatus::Queued => (IconName::Clock, cx.theme().muted_foreground),
        ItemStatus::Downloading => (IconName::Download, cx.theme().primary),
        ItemStatus::Paused => (IconName::Pause, cx.theme().warning),
        ItemStatus::Completed => (IconName::CircleCheck, cx.theme().success),
        ItemStatus::Failed => (IconName::CircleX, cx.theme().danger),
        ItemStatus::Skipped => (IconName::SkipForward, cx.theme().muted_foreground),
    };

    h_flex()
        .items_center()
        .gap_1()
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(
            div()
                .text_xs()
                .font_medium()
                .text_color(color)
                .child(item.status().label()),
        )
        .when(
            item.status() == ItemStatus::Failed && item.attempts() > 0,
            |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}回", item.attempts())),
                )
            },
        )
        .into_any_element()
}

fn url_cell(item: &QueueItem, cx: &App) -> AnyElement {
    v_flex()
        .justify_center()
        .gap_0p5()
        .min_w_0()
        .w_full()
        .child(
            div()
                .text_xs()
                .font_medium()
                .font_family(cx.theme().mono_font_family.clone())
                .truncate()
                .child(display_url(item.url())),
        )
        .when_some(item.error(), |this, error| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .truncate()
                    .child(error.clone()),
            )
        })
        .into_any_element()
}

fn quality_menu(
    model: Entity<QueueModel>,
    item: &QueueItem,
    access: SettingsAccess,
    cx: &App,
) -> AnyElement {
    let item_id = item.id();
    let current = item.quality();
    if access == SettingsAccess::ReadOnly {
        return read_only_value(current.short(), cx);
    }

    let disabled = access == SettingsAccess::Disabled;
    let tooltip = if disabled {
        "ダウンロード中は変更できません"
    } else {
        "画質を変更"
    };
    let accessibility = if disabled {
        format!("画質: {}", current.label())
    } else {
        format!("画質: {}（変更）", current.label())
    };

    Button::new(("quality", item_id))
        .outline()
        .small()
        .w_full()
        .icon(IconName::ChevronDown)
        .label(current.short())
        .tooltip(tooltip)
        .accessibility_label(accessibility)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for quality in Quality::ALL {
                let menu_model = model.clone();
                menu = menu.item(
                    PopupMenuItem::new(quality.label())
                        .checked(quality == current)
                        .on_click(move |_, _, cx| {
                            menu_model.update(cx, |model, cx| {
                                model.set_item_quality(item_id, quality);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

fn audio_menu(
    model: Entity<QueueModel>,
    item: &QueueItem,
    access: SettingsAccess,
    cx: &App,
) -> AnyElement {
    let item_id = item.id();
    let current = item.audio();
    if access == SettingsAccess::ReadOnly {
        return read_only_value(current.short(), cx);
    }

    let disabled = access == SettingsAccess::Disabled;
    let tooltip = if disabled {
        "ダウンロード中は変更できません"
    } else {
        "音質を変更"
    };
    let accessibility = if disabled {
        format!("音質: {}", current.label())
    } else {
        format!("音質: {}（変更）", current.label())
    };

    Button::new(("audio", item_id))
        .outline()
        .small()
        .w_full()
        .icon(IconName::ChevronDown)
        .label(current.short())
        .tooltip(tooltip)
        .accessibility_label(accessibility)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for audio in AudioQuality::ALL {
                let menu_model = model.clone();
                menu = menu.item(
                    PopupMenuItem::new(audio.label())
                        .checked(audio == current)
                        .on_click(move |_, _, cx| {
                            menu_model.update(cx, |model, cx| {
                                model.set_item_audio(item_id, audio);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

fn container_menu(
    model: Entity<QueueModel>,
    item: &QueueItem,
    access: SettingsAccess,
    cx: &App,
) -> AnyElement {
    let item_id = item.id();
    let current = item.container();
    let quality = item.quality();
    if access == SettingsAccess::ReadOnly {
        return read_only_value(current.label(), cx);
    }

    let disabled = access == SettingsAccess::Disabled;
    let tooltip = if disabled {
        "ダウンロード中は変更できません"
    } else {
        "コンテナを変更"
    };
    let accessibility = if disabled {
        format!("コンテナ: {}", current.label())
    } else {
        format!("コンテナ: {}（変更）", current.label())
    };

    Button::new(("container", item_id))
        .outline()
        .small()
        .w_full()
        .icon(IconName::ChevronDown)
        .label(current.label())
        .tooltip(tooltip)
        .accessibility_label(accessibility)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for container in Container::options_for(quality) {
                let container = *container;
                let menu_model = model.clone();
                menu = menu.item(
                    PopupMenuItem::new(container.label())
                        .checked(container == current)
                        .on_click(move |_, _, cx| {
                            menu_model.update(cx, |model, cx| {
                                model.set_item_container(item_id, container);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

fn destination_menu(
    model: Entity<QueueModel>,
    item: &QueueItem,
    destinations: &[String],
    access: SettingsAccess,
    cx: &App,
) -> AnyElement {
    let item_id = item.id();
    let current = item.destination().to_string();
    let label = folder_label(&current);
    if access == SettingsAccess::ReadOnly {
        return read_only_value(label, cx);
    }

    let mut destinations = destinations.to_vec();
    if !destinations.contains(&current) {
        destinations.push(current.clone());
    }
    let disabled = access == SettingsAccess::Disabled;
    let accessibility = if disabled {
        format!("DLフォルダ: {current}")
    } else {
        format!("DLフォルダ: {current}（変更）")
    };
    let tooltip = if disabled {
        "ダウンロード中は変更できません".to_string()
    } else {
        current.clone()
    };

    Button::new(("destination", item_id))
        .outline()
        .small()
        .w_full()
        .icon(IconName::ChevronDown)
        .label(label)
        .tooltip(tooltip)
        .accessibility_label(accessibility)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for destination in &destinations {
                let menu_model = model.clone();
                let value = destination.clone();
                let checked = current == *destination;
                menu = menu.item(
                    PopupMenuItem::new(destination.clone())
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            menu_model.update(cx, |model, cx| {
                                model.set_item_destination(item_id, &value);
                                cx.emit(QueueEvent::Changed);
                                cx.notify();
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

/// 設定を変更できない行で、現在値を表示する。
fn read_only_value(label: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .truncate()
        .text_xs()
        .text_color(cx.theme().foreground.opacity(0.75))
        .child(label.into())
        .into_any_element()
}

fn folder_label(path: &str) -> SharedString {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.to_string())
        .into()
}

fn progress_cell(item: &QueueItem, cx: &App) -> AnyElement {
    let color = match item.status() {
        ItemStatus::Completed => cx.theme().success,
        ItemStatus::Failed => cx.theme().danger,
        ItemStatus::Paused => cx.theme().warning,
        ItemStatus::Skipped => cx.theme().muted_foreground,
        _ => cx.theme().primary,
    };
    let progress = item.progress().clamp(0.0, 100.0);

    v_flex()
        .justify_center()
        .gap_1()
        .w_full()
        .child(
            Progress::new(("queue-progress", item.id()))
                .value(progress)
                .small()
                .color(color),
        )
        .child(
            h_flex()
                .justify_between()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{progress:.0}%"))
                .when_some(item.eta().cloned(), |this, eta| this.child(eta)),
        )
        .into_any_element()
}

fn size_cell(item: &QueueItem, cx: &App) -> AnyElement {
    v_flex()
        .justify_center()
        .w_full()
        .items_end()
        .gap_0p5()
        .child(
            div()
                .whitespace_nowrap()
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(item.downloaded().clone()),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("/ {}", item.total())),
        )
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ItemActions {
    retry_visible: bool,
    retry_enabled: bool,
    remove_enabled: bool,
    play_visible: bool,
}

impl ItemActions {
    fn new(running: bool, status: ItemStatus) -> Self {
        let retry_visible = status == ItemStatus::Failed;
        Self {
            retry_visible,
            retry_enabled: !running && retry_visible,
            remove_enabled: !running && status != ItemStatus::Downloading,
            play_visible: matches!(status, ItemStatus::Completed | ItemStatus::Skipped),
        }
    }
}

fn actions_cell(model: Entity<QueueModel>, item: &QueueItem, running: bool) -> AnyElement {
    let retry_model = model.clone();
    let remove_model = model.clone();
    let retry_id = item.id();
    let remove_id = item.id();
    let actions = ItemActions::new(running, item.status());
    let play_path = if actions.play_visible {
        item.output_path().map(str::to_string)
    } else {
        None
    };

    h_flex()
        .w_full()
        .justify_end()
        .gap_1()
        .when(actions.retry_visible, |this| {
            this.child(
                Button::new(("retry", retry_id))
                    .xsmall()
                    .icon(IconName::RotateCcw)
                    .label("再試行")
                    .tooltip(if running {
                        "実行中は再試行できません"
                    } else {
                        "再試行"
                    })
                    .accessibility_label(format!("再試行: {}", compact_url(item.url())))
                    .disabled(!actions.retry_enabled)
                    .on_click(move |_, _, cx| {
                        retry_model.update(cx, |model, cx| {
                            model.retry(retry_id);
                            cx.emit(QueueEvent::Changed);
                            cx.notify();
                        });
                    }),
            )
        })
        .child(
            Button::new(("remove", remove_id))
                .xsmall()
                .icon(IconName::X)
                .label("削除")
                .tooltip(if running {
                    "実行中は削除できません"
                } else {
                    "キューから削除"
                })
                .accessibility_label(format!("キューから削除: {}", compact_url(item.url())))
                .disabled(!actions.remove_enabled)
                .on_click(move |_, _, cx| {
                    remove_model.update(cx, |model, cx| {
                        model.remove(remove_id);
                        cx.emit(QueueEvent::Changed);
                        cx.notify();
                    });
                }),
        )
        .when_some(play_path, |this, path| {
            this.child(
                Button::new(("play", item.id()))
                    .xsmall()
                    .icon(IconName::Play)
                    .label("再生")
                    .tooltip("既定のアプリで再生")
                    .accessibility_label(format!("再生: {}", compact_url(item.url())))
                    .on_click(move |_, window, cx| {
                        if let Err(error) = open_output_file(&path) {
                            window.push_notification(
                                Notification::error(format!(
                                    "ファイルを再生できませんでした: {error}"
                                )),
                                cx,
                            );
                        }
                    }),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ItemActions, ItemStatus, can_start_site, open_output_file};

    #[test]
    fn running_disables_queue_mutations_but_allows_completed_playback() {
        for status in [
            ItemStatus::Queued,
            ItemStatus::Downloading,
            ItemStatus::Paused,
            ItemStatus::Completed,
            ItemStatus::Failed,
            ItemStatus::Skipped,
        ] {
            let actions = ItemActions::new(true, status);
            assert_eq!(actions.retry_visible, status == ItemStatus::Failed);
            assert!(!actions.retry_enabled);
            assert!(!actions.remove_enabled);
            assert_eq!(
                actions.play_visible,
                matches!(status, ItemStatus::Completed | ItemStatus::Skipped)
            );
        }
    }

    #[test]
    fn stopped_row_actions_follow_item_status() {
        assert_eq!(
            ItemActions::new(false, ItemStatus::Failed),
            ItemActions {
                retry_visible: true,
                retry_enabled: true,
                remove_enabled: true,
                play_visible: false,
            }
        );
        assert_eq!(
            ItemActions::new(false, ItemStatus::Skipped),
            ItemActions {
                retry_visible: false,
                retry_enabled: false,
                remove_enabled: true,
                play_visible: true,
            }
        );
        assert_eq!(
            ItemActions::new(false, ItemStatus::Completed),
            ItemActions {
                retry_visible: false,
                retry_enabled: false,
                remove_enabled: true,
                play_visible: true,
            }
        );
        assert_eq!(
            ItemActions::new(false, ItemStatus::Queued),
            ItemActions {
                retry_visible: false,
                retry_enabled: false,
                remove_enabled: true,
                play_visible: false,
            }
        );
        assert_eq!(
            ItemActions::new(false, ItemStatus::Paused),
            ItemActions {
                retry_visible: false,
                retry_enabled: false,
                remove_enabled: true,
                play_visible: false,
            }
        );
        assert_eq!(
            ItemActions::new(false, ItemStatus::Downloading),
            ItemActions {
                retry_visible: false,
                retry_enabled: false,
                remove_enabled: false,
                play_visible: false,
            }
        );
    }

    #[test]
    fn updating_disables_site_start_and_pause() {
        assert!(can_start_site(false, true, false));
        assert!(can_start_site(true, false, false));
        assert!(!can_start_site(false, true, true));
        assert!(!can_start_site(true, false, true));
        assert!(!can_start_site(false, false, false));
    }

    #[test]
    fn opening_a_missing_output_file_returns_an_error() {
        let path = std::env::temp_dir().join(format!("tmd-missing-{}.mp4", std::process::id()));
        assert!(open_output_file(path.to_str().unwrap()).is_err());
    }
}
