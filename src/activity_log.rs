//! サイトごとのキューに表示するログ。

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, div};

use crate::model::{LogEntry, LogLevel};

/// サイトのログ一覧をアコーディオン内に表示する。
pub fn render_site_logs(logs: &[LogEntry], cx: &App) -> AnyElement {
    if logs.is_empty() {
        return div()
            .px_3()
            .py_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("このサイトのログはまだありません。")
            .into_any_element();
    }

    v_flex()
        .w_full()
        .gap_1()
        .px_3()
        .py_2()
        .children(logs.iter().map(|entry| log_row(entry, cx)))
        .into_any_element()
}

/// クリップボードへコピーするためのログ全文。
pub fn logs_as_text(logs: &[LogEntry]) -> String {
    let mut text = String::new();
    for entry in logs {
        text.push_str(entry.time());
        text.push(' ');
        text.push_str(entry.level().label());
        text.push(' ');
        text.push_str(entry.message());
        text.push('\n');
    }
    text
}

fn log_row(entry: &LogEntry, cx: &App) -> AnyElement {
    let level_color = match entry.level() {
        LogLevel::Info => cx.theme().muted_foreground,
        LogLevel::Warn => cx.theme().warning,
        LogLevel::Error => cx.theme().danger,
    };

    h_flex()
        .items_start()
        .gap_2()
        .py_0p5()
        .child(
            div()
                .w_16()
                .flex_shrink_0()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(cx.theme().muted_foreground)
                .child(entry.time().clone()),
        )
        .child(
            div()
                .w_11()
                .flex_shrink_0()
                .text_xs()
                .font_semibold()
                .text_color(level_color)
                .child(entry.level().label()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(cx.theme().foreground)
                .child(entry.message().clone()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{QueueModel, Site};

    #[test]
    fn formats_logs_for_the_clipboard() {
        let model = QueueModel::demo();
        let logs = model.logs_for_site(Site::YouTube);
        assert!(!logs.is_empty());

        let text = logs_as_text(&logs);

        assert!(text.contains(" INFO "));
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), logs.len());
    }
}
