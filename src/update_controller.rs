//! アプリ本体の更新状態とダイアログ。
//!
//! 通信とファイル操作は [`crate::updater`] が担い、ここでは進捗をUI向けの状態へ
//! 変換して、タイトルバー・About・確認ダイアログへ反映する。

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    v_flex,
};
use gpui_kit::*;

use crate::updater::{self, InstallEvent, UpdateInfo};

/// インストール処理の段階。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallPhase {
    Downloading { received: u64, total: Option<u64> },
    Verifying,
    Extracting,
    Placing,
}

impl InstallPhase {
    pub fn label(&self) -> String {
        match self {
            Self::Downloading { received, total } => match total.filter(|total| *total > 0) {
                Some(total) => format!(
                    "ダウンロード {}%",
                    ((*received as f32 / total as f32) * 100.0).clamp(0.0, 100.0) as u32
                ),
                None => "ダウンロード中".to_string(),
            },
            Self::Verifying => "ダウンロードを検証中".to_string(),
            Self::Extracting => "展開中".to_string(),
            Self::Placing => "アプリを配置中".to_string(),
        }
    }

    pub fn percent(&self) -> Option<f32> {
        match self {
            Self::Downloading { received, total } => total
                .filter(|total| *total > 0)
                .map(|total| ((*received as f32 / total as f32) * 100.0).clamp(0.0, 100.0)),
            _ => None,
        }
    }
}

/// 更新確認・インストールの状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Installing(InstallPhase),
    Failed(String),
}

impl UpdateState {
    pub fn is_checking(&self) -> bool {
        matches!(self, Self::Checking)
    }

    pub fn is_installing(&self) -> bool {
        matches!(self, Self::Installing(_))
    }
}

/// 更新状態を保持し、更新確認・インストールを開始する。
pub struct UpdateController {
    state: UpdateState,
    available: Option<UpdateInfo>,
    no_update_visible: bool,
    hide_task: Option<Task<()>>,
}

impl UpdateController {
    pub fn new() -> Self {
        Self {
            state: UpdateState::Idle,
            available: None,
            no_update_visible: false,
            hide_task: None,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn is_checking(&self) -> bool {
        self.state.is_checking()
    }

    pub fn is_installing(&self) -> bool {
        self.state.is_installing()
    }

    /// 更新なしのタイトルバー表示を出しているか。
    pub fn no_update_visible(&self) -> bool {
        self.no_update_visible
    }

    pub fn status_text(&self) -> String {
        match &self.state {
            UpdateState::Idle => "更新を確認していません。".to_string(),
            UpdateState::Checking => "更新を確認しています…".to_string(),
            UpdateState::UpToDate => "お使いのバージョンは最新です。".to_string(),
            UpdateState::Available(info) => {
                format!("v{} にアップデートできます。", info.version)
            }
            UpdateState::Installing(phase) => format!("インストール中: {}", phase.label()),
            UpdateState::Failed(message) => {
                format!("インストールに失敗しました。{message}")
            }
        }
    }

    /// 起動時のバックグラウンド確認。ダイアログは表示しない。
    pub fn check_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state, UpdateState::Idle) {
            self.spawn_check(window, cx, false);
        }
    }

    /// メニュー・Aboutからの手動確認。
    pub fn check_manually(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_installing() || self.is_checking() || window.has_active_dialog(cx) {
            return;
        }
        self.spawn_check(window, cx, true);
    }

    fn spawn_check(&mut self, window: &mut Window, cx: &mut Context<Self>, manual: bool) {
        let previous = self.state.clone();
        let previous_no_update_visible = self.no_update_visible;
        self.state = UpdateState::Checking;
        self.no_update_visible = false;
        self.cancel_hide();
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            let result = updater::check_for_update();
            let _ = sender.send_blocking(result);
        });

        cx.spawn_in(window, async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or_else(|error| Err(format!("更新確認を中断しました: {error}")));
            this.update_in(cx, |this, window, cx| {
                this.finish_check(
                    result,
                    manual,
                    previous,
                    previous_no_update_visible,
                    window,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    fn finish_check(
        &mut self,
        result: Result<Option<UpdateInfo>, String>,
        manual: bool,
        previous: UpdateState,
        previous_no_update_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Some(info)) => {
                self.available = Some(info.clone());
                self.state = UpdateState::Available(info.clone());
                self.cancel_hide();
                if manual {
                    self.open_available_dialog(info, window, cx);
                }
            }
            Ok(None) => {
                self.available = None;
                if manual {
                    self.state = UpdateState::UpToDate;
                    self.no_update_visible = true;
                    self.schedule_hide(cx);
                    self.open_up_to_date_dialog(window, cx);
                } else {
                    self.state = UpdateState::Idle;
                }
            }
            Err(message) => {
                if manual {
                    self.state = previous;
                    self.no_update_visible = previous_no_update_visible;
                    if matches!(self.state, UpdateState::UpToDate) && previous_no_update_visible {
                        self.schedule_hide(cx);
                    }
                    self.open_error_dialog(
                        "アップデートを確認できませんでした",
                        &message,
                        window,
                        cx,
                    );
                } else {
                    self.state = UpdateState::Idle;
                }
            }
        }
        cx.notify();
    }

    /// 更新がある状態から、確認ダイアログを開く。
    pub fn open_available_dialog(
        &mut self,
        info: UpdateInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let controller = cx.entity();

        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            let install_controller = controller.clone();
            dialog
                .title("アップデートが有ります")
                .width(px(460.))
                .centered()
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .child(format!("v{} が利用可能です。", info.version)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(dialog_cx.theme().muted_foreground)
                                .child(
                                    "ダウンロード後にアプリを置き換えて再起動します。作業中のダウンロードは中断されます。",
                                ),
                        ),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("later-update")
                                .label("後で")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("install-update")
                                .primary()
                                .label("インストール")
                                .on_click(move |_, window, cx| {
                                    install_controller.update(cx, |controller, cx| {
                                        controller.install(window, cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn open_up_to_date_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title("お使いのバージョンは最新です")
                .description(format!(
                    "現在のバージョンは v{} です。",
                    crate::app_settings::APP_VERSION
                ))
                .show_cancel(false)
                .ok_text("閉じる")
                .on_ok(|_, _, _| true)
        });
    }

    fn open_error_dialog(
        &self,
        title: &'static str,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = message.to_string();
        window.open_alert_dialog(cx, move |alert, _, dialog_cx| {
            alert
                .icon(
                    Icon::new(IconName::CircleAlert)
                        .small()
                        .text_color(dialog_cx.theme().danger),
                )
                .title(title)
                .description(message.clone())
                .show_cancel(false)
                .ok_text("閉じる")
                .on_ok(|_, _, _| true)
        });
    }

    /// インストールを開始する。失敗後の再実行にも使う。
    pub fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_installing() {
            return;
        }
        let Some(info) = self.available.clone() else {
            return;
        };
        self.cancel_hide();
        self.state = UpdateState::Installing(InstallPhase::Downloading {
            received: 0,
            total: None,
        });
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || updater::install_update(info, sender));

        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let terminal = matches!(
                    event,
                    InstallEvent::Installed { .. }
                        | InstallEvent::Revealed { .. }
                        | InstallEvent::Failed(_)
                );
                this.update_in(cx, |this, window, cx| {
                    this.finish_install(event, window, cx);
                })
                .ok();
                if terminal {
                    break;
                }
            }
        })
        .detach();
    }

    /// 失敗後に最初からやり直す。
    pub fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state, UpdateState::Failed(_)) {
            self.install(window, cx);
        }
    }

    /// 「アップデートは有りません」表示を閉じる。
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, UpdateState::UpToDate) {
            self.no_update_visible = false;
            self.cancel_hide();
            cx.notify();
        }
    }

    fn finish_install(&mut self, event: InstallEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            InstallEvent::Downloading { received, total } => {
                self.state = UpdateState::Installing(InstallPhase::Downloading { received, total });
            }
            InstallEvent::Verifying => {
                self.state = UpdateState::Installing(InstallPhase::Verifying);
            }
            InstallEvent::Extracting => {
                self.state = UpdateState::Installing(InstallPhase::Extracting);
            }
            InstallEvent::Placing => {
                self.state = UpdateState::Installing(InstallPhase::Placing);
            }
            InstallEvent::Installed { app: _ } => {
                self.state = UpdateState::Idle;
                self.available = None;
                cx.notify();
                cx.quit();
                return;
            }
            InstallEvent::Revealed { app } => {
                self.state = UpdateState::Idle;
                cx.reveal_path(&app);
                window.push_notification(
                    Notification::info(
                        "開発ビルドのため自動更新できません。Finderで表示したアプリを確認してください。",
                    ),
                    cx,
                );
            }
            InstallEvent::Failed(message) => {
                self.state = UpdateState::Failed(message.clone());
                self.open_error_dialog("インストールに失敗しました", &message, window, cx);
            }
        }
        cx.notify();
    }

    fn schedule_hide(&mut self, cx: &mut Context<Self>) {
        self.hide_task = Some(cx.spawn(async move |this, cx| {
            smol::Timer::after(Duration::from_secs(10)).await;
            this.update(cx, |this, cx| {
                if matches!(this.state, UpdateState::UpToDate) {
                    this.no_update_visible = false;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn cancel_hide(&mut self) {
        self.hide_task = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallPhase, UpdateController, UpdateState};
    use crate::updater::UpdateInfo;

    fn info() -> UpdateInfo {
        UpdateInfo {
            version: "0.2.0".to_string(),
            url: "https://example.com/app.zip".to_string(),
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn formats_download_phase() {
        let phase = InstallPhase::Downloading {
            received: 50,
            total: Some(100),
        };
        assert_eq!(phase.label(), "ダウンロード 50%");
        assert_eq!(phase.percent(), Some(50.0));

        let phase = InstallPhase::Downloading {
            received: 50,
            total: None,
        };
        assert_eq!(phase.label(), "ダウンロード中");
        assert_eq!(phase.percent(), None);
    }

    #[test]
    fn reports_installing_only_for_the_install_state() {
        let mut controller = UpdateController::new();
        assert!(!controller.is_installing());
        controller.state = UpdateState::Installing(InstallPhase::Verifying);
        assert!(controller.is_installing());
    }

    #[test]
    fn shows_the_available_version() {
        let controller = UpdateController {
            state: UpdateState::Available(info()),
            available: Some(info()),
            no_update_visible: false,
            hide_task: None,
        };
        assert!(controller.status_text().contains("0.2.0"));
    }
}
