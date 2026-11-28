//! アプリ本体とライブラリの更新状態・ダイアログ。
//!
//! 通信とファイル操作は [`crate::updater`] と [`crate::tools`] が担い、ここでは
//! 2種類の更新を独立した状態として保持し、更新通知・About・確認ダイアログへ反映する。

use std::{path::PathBuf, time::Duration};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    v_flex,
};
use gpui_kit::*;

use crate::tools::{self, BootstrapEvent, ToolEntry, ToolPaths};
use crate::updater::{self, AppPlacement, InstallEvent, UpdateInfo};

gpui_kit::actions!([InstallAvailableUpdates]);

/// アプリ本体のインストール処理の段階。
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

    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Downloading { .. } => "ダウンロード中",
            Self::Verifying => "ダウンロードを検証中",
            Self::Extracting => "展開中",
            Self::Placing => "アプリを配置中",
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

/// アプリ本体の更新確認・インストールの状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    CheckFailed(String),
    Available(UpdateInfo),
    Installing(InstallPhase),
    Installed {
        app: PathBuf,
        rollback: AppPlacement,
    },
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

/// ライブラリのインストール処理の段階。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryInstallPhase {
    Downloading {
        tool: String,
        received: u64,
        total: Option<u64>,
        step: usize,
        steps: usize,
    },
    Installing {
        tool: String,
        step: usize,
        steps: usize,
    },
}

impl LibraryInstallPhase {
    pub fn label(&self) -> String {
        match self {
            Self::Downloading {
                tool,
                received,
                total,
                step,
                steps,
            } => match total.filter(|total| *total > 0) {
                Some(total) => format!(
                    "{tool} をダウンロード {}% ({step}/{steps})",
                    ((*received as f32 / total as f32) * 100.0).clamp(0.0, 100.0) as u32
                ),
                None => format!("{tool} をダウンロード中 ({step}/{steps})"),
            },
            Self::Installing { tool, step, steps } => {
                format!("{tool} を配置中 ({step}/{steps})")
            }
        }
    }

    pub fn status_label(&self) -> String {
        match self {
            Self::Downloading {
                tool, step, steps, ..
            } => format!("{tool} をダウンロード中 ({step}/{steps})"),
            Self::Installing { tool, step, steps } => {
                format!("{tool} を配置中 ({step}/{steps})")
            }
        }
    }

    pub fn percent(&self) -> Option<f32> {
        match self {
            Self::Downloading {
                received, total, ..
            } => total
                .filter(|total| *total > 0)
                .map(|total| ((*received as f32 / total as f32) * 100.0).clamp(0.0, 100.0)),
            Self::Installing { .. } => None,
        }
    }

    pub fn step(&self) -> usize {
        match self {
            Self::Downloading { step, .. } | Self::Installing { step, .. } => *step,
        }
    }
}

/// ライブラリの更新確認・インストールの状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryUpdateState {
    Idle,
    Checking,
    UpToDate,
    CheckFailed(String),
    Available(Vec<ToolEntry>),
    Installing(LibraryInstallPhase),
    Failed(String),
}

impl LibraryUpdateState {
    pub fn is_checking(&self) -> bool {
        matches!(self, Self::Checking)
    }

    pub fn is_installing(&self) -> bool {
        matches!(self, Self::Installing(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallPlan {
    None,
    App,
    Libraries,
    LibrariesThenApp,
}

#[derive(Clone, Debug)]
struct PreviousUpdateState {
    app: UpdateState,
    library: LibraryUpdateState,
}

/// 更新状態を保持し、アプリ本体とライブラリの確認・インストールを調整する。
pub struct UpdateController {
    state: UpdateState,
    library_state: LibraryUpdateState,
    available: Option<UpdateInfo>,
    available_tools: Vec<ToolEntry>,
    no_update_visible: bool,
    app_notification_dismissed: bool,
    library_notification_dismissed: bool,
    hide_task: Option<Task<()>>,
    install_app_after_library: bool,
    library_background_started: bool,
    library_check_completed: bool,
    library_check_unavailable: bool,
    dialog_plan: Option<InstallPlan>,
}

impl UpdateController {
    pub fn new() -> Self {
        Self {
            state: UpdateState::Idle,
            library_state: LibraryUpdateState::Idle,
            available: None,
            available_tools: Vec::new(),
            no_update_visible: false,
            app_notification_dismissed: false,
            library_notification_dismissed: false,
            hide_task: None,
            install_app_after_library: false,
            library_background_started: false,
            library_check_completed: false,
            library_check_unavailable: false,
            dialog_plan: None,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn library_state(&self) -> &LibraryUpdateState {
        &self.library_state
    }

    pub fn is_checking(&self) -> bool {
        self.state.is_checking()
    }

    pub fn is_library_checking(&self) -> bool {
        self.library_state.is_checking()
    }

    pub fn is_installing(&self) -> bool {
        self.state.is_installing()
    }

    pub fn is_library_installing(&self) -> bool {
        self.library_state.is_installing()
    }

    pub fn is_busy(&self) -> bool {
        self.is_checking()
            || self.is_library_checking()
            || self.is_installing()
            || self.is_library_installing()
    }

    pub fn has_app_update(&self) -> bool {
        matches!(self.state, UpdateState::Available(_))
    }

    pub fn has_library_update(&self) -> bool {
        matches!(self.library_state, LibraryUpdateState::Available(_))
    }

    /// アプリ更新の通知を表示できるか。閉じても更新自体の状態は保持する。
    pub fn is_app_update_notification_visible(&self) -> bool {
        !self.app_notification_dismissed
    }

    /// ライブラリ更新の通知を表示できるか。閉じても更新自体の状態は保持する。
    pub fn is_library_update_notification_visible(&self) -> bool {
        !self.library_notification_dismissed
    }

    /// 更新通知に表示する更新ありの文案。
    pub fn available_update_label(&self) -> Option<&'static str> {
        if (!self.library_check_completed && !self.library_check_unavailable)
            || matches!(self.state, UpdateState::CheckFailed(_))
            || (matches!(self.library_state, LibraryUpdateState::CheckFailed(_))
                && !self.library_check_unavailable)
        {
            return None;
        }
        Self::label_for(
            self.has_app_update() && self.is_app_update_notification_visible(),
            self.has_library_update() && self.is_library_update_notification_visible(),
        )
    }

    fn label_for(app: bool, library: bool) -> Option<&'static str> {
        match (app, library) {
            (true, true) => Some("アプリとライブラリの更新が有ります"),
            (true, false) => Some("アプリの更新が有ります"),
            (false, true) => Some("ライブラリの更新が有ります"),
            (false, false) => None,
        }
    }

    fn install_plan(&self) -> InstallPlan {
        match (self.has_app_update(), self.has_library_update()) {
            (true, true) => InstallPlan::LibrariesThenApp,
            (true, false) => InstallPlan::App,
            (false, true) => InstallPlan::Libraries,
            (false, false) => InstallPlan::None,
        }
    }

    /// 更新なしの通知を出しているか。
    pub fn no_update_visible(&self) -> bool {
        self.no_update_visible
    }

    /// 起動時のアプリ更新確認。ダイアログは表示しない。
    pub fn check_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.state, UpdateState::Idle) {
            self.spawn_app_check(window, cx);
        }
    }

    /// ツール準備完了後のライブラリ更新確認。ダイアログは表示しない。
    pub fn check_libraries_background(&mut self, cx: &mut Context<Self>) {
        if self.library_background_started
            || !matches!(self.library_state, LibraryUpdateState::Idle)
        {
            return;
        }
        self.library_background_started = true;
        self.spawn_library_check(cx);
    }

    /// ツール準備が失敗し、ライブラリ確認を開始できない状態を記録する。
    pub fn mark_library_check_unavailable(&mut self, cx: &mut Context<Self>) {
        if self.library_check_completed {
            return;
        }
        self.library_check_completed = true;
        self.library_check_unavailable = true;
        self.library_state = LibraryUpdateState::CheckFailed(
            "ツール準備が完了していないため確認できません。".to_string(),
        );
        cx.notify();
    }

    /// メニュー・Aboutからの手動確認。アプリとライブラリの両方を確認する。
    pub fn check_manually(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() || window.has_active_dialog(cx) {
            return;
        }
        if matches!(self.state, UpdateState::Installed { .. }) {
            window.push_notification(
                Notification::info("アプリの更新が完了しています。再起動してください。"),
                cx,
            );
            return;
        }

        let previous = PreviousUpdateState {
            app: self.state.clone(),
            library: self.library_state.clone(),
        };
        self.state = UpdateState::Checking;
        self.library_state = LibraryUpdateState::Checking;
        self.no_update_visible = false;
        self.app_notification_dismissed = false;
        self.library_notification_dismissed = false;
        self.cancel_hide();
        cx.notify();

        let (app_sender, app_receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            let _ = app_sender.send_blocking(updater::check_for_update());
        });
        let (library_sender, library_receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            let _ =
                library_sender.send_blocking(tools::check_for_tool_updates(ToolPaths::from_env()));
        });

        cx.spawn_in(window, async move |this, cx| {
            let app_result = app_receiver
                .recv()
                .await
                .unwrap_or_else(|error| Err(format!("アプリの更新確認を中断しました: {error}")));
            let library_result = library_receiver.recv().await.unwrap_or_else(|error| {
                Err(format!("ライブラリの更新確認を中断しました: {error}"))
            });
            this.update_in(cx, |this, window, cx| {
                this.finish_manual_check(app_result, library_result, previous, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn spawn_app_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = UpdateState::Checking;
        self.app_notification_dismissed = false;
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            let _ = sender.send_blocking(updater::check_for_update());
        });

        cx.spawn_in(window, async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or_else(|error| Err(format!("更新確認を中断しました: {error}")));
            this.update_in(cx, |this, _, cx| {
                this.finish_app_background_check(result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_app_background_check(
        &mut self,
        result: Result<Option<UpdateInfo>, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Some(info)) => {
                self.available = Some(info.clone());
                self.state = UpdateState::Available(info);
            }
            Ok(None) => {
                self.available = None;
                self.state = UpdateState::UpToDate;
            }
            Err(message) => self.state = UpdateState::CheckFailed(message),
        }
        cx.notify();
    }

    fn spawn_library_check(&mut self, cx: &mut Context<Self>) {
        self.library_state = LibraryUpdateState::Checking;
        self.library_notification_dismissed = false;
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            let _ = sender.send_blocking(tools::check_for_tool_updates(ToolPaths::from_env()));
        });

        cx.spawn(async move |this, cx| {
            let result = receiver.recv().await.unwrap_or_else(|error| {
                Err(format!("ライブラリの更新確認を中断しました: {error}"))
            });
            this.update(cx, |this, cx| {
                this.finish_library_background_check(result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_library_background_check(
        &mut self,
        result: Result<Vec<ToolEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        self.library_check_completed = true;
        match result {
            Ok(entries) if entries.is_empty() => {
                self.available_tools.clear();
                self.library_state = LibraryUpdateState::UpToDate;
            }
            Ok(entries) => {
                self.available_tools = entries.clone();
                self.library_state = LibraryUpdateState::Available(entries);
            }
            Err(message) => self.library_state = LibraryUpdateState::CheckFailed(message),
        }
        cx.notify();
    }

    fn finish_manual_check(
        &mut self,
        app_result: Result<Option<UpdateInfo>, String>,
        library_result: Result<Vec<ToolEntry>, String>,
        previous: PreviousUpdateState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library_check_completed = true;
        let mut errors = Vec::new();

        match app_result {
            Ok(Some(info)) => {
                self.available = Some(info.clone());
                self.state = UpdateState::Available(info);
            }
            Ok(None) => {
                self.available = None;
                self.state = UpdateState::UpToDate;
            }
            Err(message) => {
                self.state = previous.app.clone();
                errors.push(format!("アプリ: {message}"));
            }
        }

        match library_result {
            Ok(entries) if entries.is_empty() => {
                self.available_tools.clear();
                self.library_state = LibraryUpdateState::UpToDate;
            }
            Ok(entries) => {
                self.available_tools = entries.clone();
                self.library_state = LibraryUpdateState::Available(entries);
            }
            Err(message) => {
                self.library_state = previous.library.clone();
                errors.push(format!("ライブラリ: {message}"));
            }
        }

        if errors.is_empty() && self.available_update_label().is_none() {
            self.no_update_visible = true;
            self.schedule_hide(cx);
            self.open_up_to_date_dialog(window, cx);
        } else if errors.is_empty() {
            self.open_available_dialog(window, cx);
        } else {
            self.hide_notifications_after_failed_check();
            self.open_error_dialog(
                "アップデートを確認できませんでした",
                &errors.join("\n"),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 利用可能な更新を確認ダイアログで表示する。
    pub fn open_available_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.available_update_label().is_none() {
            return;
        }
        self.open_dialog_for_plan(self.install_plan(), window, cx);
    }

    /// アプリ本体だけの確認ダイアログを表示する。
    pub fn open_app_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_app_update() {
            self.open_dialog_for_plan(InstallPlan::App, window, cx);
        }
    }

    /// ライブラリだけの確認ダイアログを表示する。
    pub fn open_library_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_library_update() {
            self.open_dialog_for_plan(InstallPlan::Libraries, window, cx);
        }
    }

    fn open_dialog_for_plan(
        &mut self,
        plan: InstallPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if plan == InstallPlan::None || window.has_active_dialog(cx) {
            return;
        }
        self.dialog_plan = Some(plan);
        let app_info = if matches!(plan, InstallPlan::App | InstallPlan::LibrariesThenApp) {
            self.available.clone()
        } else {
            None
        };
        let library_entries =
            if matches!(plan, InstallPlan::Libraries | InstallPlan::LibrariesThenApp) {
                self.available_tools.clone()
            } else {
                Vec::new()
            };

        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            let mut details = Vec::new();
            if let Some(info) = &app_info {
                details.push(
                    div()
                        .text_sm()
                        .child(format!(
                            "アプリ: v{} → v{}",
                            crate::app_settings::APP_VERSION,
                            info.version
                        ))
                        .into_any_element(),
                );
            }
            if !library_entries.is_empty() {
                let versions = library_entries
                    .iter()
                    .map(|entry| format!("{} {}", entry.name, entry.version))
                    .collect::<Vec<_>>()
                    .join("、");
                details.push(div().text_sm().child(format!("ライブラリ: {versions}")).into_any_element());
            }
            let description = if app_info.is_some() && !library_entries.is_empty() {
                "ライブラリを更新した後、アプリを置き換えます。完了後は任意のタイミングで再起動できます。作業中のダウンロードは中断されます。"
            } else if app_info.is_some() {
                "ダウンロード後にアプリを置き換えます。完了後は任意のタイミングで再起動できます。作業中のダウンロードは中断されます。"
            } else {
                "ライブラリを更新します。アプリの再起動は不要です。"
            };

            dialog
                .title("アップデートが有ります")
                .width(px(520.))
                .centered()
                .child(
                    v_flex()
                        .gap_2()
                        .children(details)
                        .child(
                            div()
                                .text_xs()
                                .text_color(dialog_cx.theme().muted_foreground)
                                .child(description),
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
                            Button::new("install-available-updates")
                                .primary()
                                .label("インストール")
                                .on_click(|_, window, cx| {
                                    window.close_dialog(cx);
                                    window.dispatch_action(
                                        InstallAvailableUpdates.boxed_clone(),
                                        cx,
                                    );
                                }),
                        ),
                )
        });
    }

    fn open_up_to_date_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title("お使いのアプリとライブラリは最新です")
                .description(format!(
                    "現在のアプリバージョンは v{} です。",
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

    /// 確認ダイアログから表示時点の更新計画を実行する。
    pub fn install_dialog_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.dialog_plan.take() else {
            return;
        };
        self.install_available(plan, window, cx);
    }

    /// ダイアログ表示時点の更新計画を確認してからまとめて更新する。
    fn install_available(
        &mut self,
        expected: InstallPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        if self.install_plan() != expected {
            self.open_available_dialog(window, cx);
            return;
        }
        match expected {
            InstallPlan::LibrariesThenApp => {
                self.install_app_after_library = true;
                self.start_library_install(window, cx);
            }
            InstallPlan::Libraries => {
                self.install_app_after_library = false;
                self.start_library_install(window, cx);
            }
            InstallPlan::App => self.start_app_install(window, cx),
            InstallPlan::None => {}
        }
    }

    fn start_app_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_installing() {
            return;
        }
        let Some(info) = self.available.clone() else {
            return;
        };
        self.cancel_hide();
        self.app_notification_dismissed = false;
        self.state = UpdateState::Installing(InstallPhase::Downloading {
            received: 0,
            total: None,
        });
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || updater::install_update(info, sender));

        cx.spawn_in(window, async move |this, cx| {
            let mut terminal_received = false;
            while let Ok(event) = receiver.recv().await {
                terminal_received = matches!(
                    event,
                    InstallEvent::Installed { .. }
                        | InstallEvent::Revealed { .. }
                        | InstallEvent::Failed(_)
                );
                this.update_in(cx, |this, window, cx| {
                    this.finish_app_install(event, window, cx);
                })
                .ok();
                if terminal_received {
                    break;
                }
            }
            if !terminal_received {
                this.update_in(cx, |this, window, cx| {
                    this.finish_app_install(
                        InstallEvent::Failed("アプリの更新処理が途中で終了しました。".to_string()),
                        window,
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    fn start_library_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_library_installing() || self.available_tools.is_empty() {
            return;
        }
        let entries = self.available_tools.clone();
        self.library_notification_dismissed = false;
        self.library_state = LibraryUpdateState::Installing(LibraryInstallPhase::Downloading {
            tool: entries[0].name.clone(),
            received: 0,
            total: None,
            step: 1,
            steps: entries.len(),
        });
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        std::thread::spawn(move || {
            tools::install_tool_updates(ToolPaths::from_env(), entries, sender);
        });

        cx.spawn_in(window, async move |this, cx| {
            let mut terminal_received = false;
            while let Ok(event) = receiver.recv().await {
                terminal_received =
                    matches!(event, BootstrapEvent::Ready | BootstrapEvent::Failed(_));
                this.update_in(cx, |this, window, cx| {
                    this.finish_library_install(event, window, cx);
                })
                .ok();
                if terminal_received {
                    break;
                }
            }
            if !terminal_received {
                this.update_in(cx, |this, window, cx| {
                    this.finish_library_install(
                        BootstrapEvent::Failed(
                            "ライブラリの更新処理が途中で終了しました。".to_string(),
                        ),
                        window,
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    fn remove_completed_library_updates(&mut self) {
        let completed = match &self.library_state {
            LibraryUpdateState::Installing(phase) => phase.step().saturating_sub(1),
            _ => 0,
        }
        .min(self.available_tools.len().saturating_sub(1));
        if completed > 0 {
            self.available_tools.drain(..completed);
        }
    }

    fn finish_library_install(
        &mut self,
        event: BootstrapEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BootstrapEvent::Checking => {}
            BootstrapEvent::Downloading {
                tool,
                received,
                total,
                step,
                steps,
            } => {
                self.library_state =
                    LibraryUpdateState::Installing(LibraryInstallPhase::Downloading {
                        tool,
                        received,
                        total,
                        step,
                        steps,
                    });
            }
            BootstrapEvent::Installing { tool, step, steps } => {
                self.library_state =
                    LibraryUpdateState::Installing(LibraryInstallPhase::Installing {
                        tool,
                        step,
                        steps,
                    });
            }
            BootstrapEvent::Ready => {
                self.available_tools.clear();
                self.library_state = LibraryUpdateState::UpToDate;
                if self.install_app_after_library {
                    self.install_app_after_library = false;
                    self.start_app_install(window, cx);
                    return;
                }
                window.push_notification(Notification::info("ライブラリを更新しました。"), cx);
            }
            BootstrapEvent::Failed(message) => {
                self.remove_completed_library_updates();
                self.install_app_after_library = false;
                self.library_state = LibraryUpdateState::Failed(message.clone());
                self.open_error_dialog("ライブラリの更新に失敗しました", &message, window, cx);
            }
        }
        cx.notify();
    }

    /// 配置済みの新しいアプリを起動し、現在のプロセスを終了する。
    pub fn relaunch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let UpdateState::Installed { app, rollback } = self.state.clone() else {
            return;
        };
        if let Err(message) = updater::launch_app(&app) {
            let message = match rollback.restore() {
                Ok(()) => format!("{message}（旧アプリを復元しました）"),
                Err(restore_error) => {
                    format!("{message}（旧アプリの復元に失敗しました: {restore_error}）")
                }
            };
            self.state = UpdateState::Failed(message.clone());
            self.app_notification_dismissed = false;
            self.open_error_dialog("再起動できませんでした", &message, window, cx);
            cx.notify();
        } else {
            cx.quit();
        }
    }

    /// アプリ本体の失敗した更新をやり直す。
    pub fn retry_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() || !matches!(self.state, UpdateState::Failed(_)) {
            return;
        }
        self.start_app_install(window, cx);
    }

    /// ライブラリの失敗した更新をやり直す。
    pub fn retry_libraries(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() || !matches!(self.library_state, LibraryUpdateState::Failed(_)) {
            return;
        }
        self.install_app_after_library = false;
        self.start_library_install(window, cx);
    }

    /// 失敗した更新をやり直す。
    pub fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        if matches!(self.library_state, LibraryUpdateState::Failed(_)) {
            self.install_app_after_library = self.available.is_some()
                && matches!(
                    self.state,
                    UpdateState::Available(_) | UpdateState::Failed(_)
                );
            self.start_library_install(window, cx);
        } else if matches!(self.state, UpdateState::Failed(_)) {
            self.start_app_install(window, cx);
        }
    }

    fn hide_notifications_after_failed_check(&mut self) {
        self.no_update_visible = false;
        self.app_notification_dismissed = true;
        self.library_notification_dismissed = true;
        self.cancel_hide();
    }

    /// 更新通知を閉じる。確認中・インストール中は、進捗を失わないよう無視する。
    pub fn dismiss_notification(&mut self, cx: &mut Context<Self>) {
        if self.dismiss_notification_state() {
            cx.notify();
        }
    }

    fn dismiss_notification_state(&mut self) -> bool {
        if self.is_busy() {
            return false;
        }

        let mut changed = false;
        if matches!(self.library_state, LibraryUpdateState::Failed(_)) {
            self.library_notification_dismissed = true;
            changed = true;
            // 失敗通知の下にアプリの更新通知を再表示しない。
            if matches!(
                self.state,
                UpdateState::Failed(_)
                    | UpdateState::UpToDate
                    | UpdateState::Available(_)
                    | UpdateState::Installed { .. }
            ) {
                self.app_notification_dismissed = true;
            }
        } else if matches!(self.state, UpdateState::Failed(_)) {
            self.app_notification_dismissed = true;
            changed = true;
        } else if matches!(self.state, UpdateState::Installed { .. }) {
            self.app_notification_dismissed = true;
            self.no_update_visible = false;
            self.cancel_hide();
            changed = true;
        } else if self.has_app_update() || self.has_library_update() {
            self.app_notification_dismissed = self.has_app_update();
            self.library_notification_dismissed = self.has_library_update();
            changed = self.app_notification_dismissed || self.library_notification_dismissed;
        } else if self.no_update_visible
            && matches!(self.state, UpdateState::UpToDate)
            && matches!(self.library_state, LibraryUpdateState::UpToDate)
        {
            self.no_update_visible = false;
            self.cancel_hide();
            changed = true;
        }

        if changed
            && !(matches!(self.state, UpdateState::UpToDate)
                && matches!(self.library_state, LibraryUpdateState::UpToDate))
        {
            self.no_update_visible = false;
            self.cancel_hide();
        }

        changed
    }

    fn finish_app_install(
        &mut self,
        event: InstallEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            InstallEvent::Installed { app, rollback } => {
                self.state = UpdateState::Installed { app, rollback };
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
                if matches!(this.state, UpdateState::UpToDate)
                    && matches!(this.library_state, LibraryUpdateState::UpToDate)
                {
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
    use std::path::PathBuf;

    use super::{
        InstallPhase, InstallPlan, LibraryInstallPhase, LibraryUpdateState, UpdateController,
        UpdateState,
    };
    use crate::tools::ToolEntry;
    use crate::updater::UpdateInfo;

    fn info() -> UpdateInfo {
        UpdateInfo {
            version: "0.2.0".to_string(),
            url: "https://example.com/app.zip".to_string(),
            sha256: "a".repeat(64),
        }
    }

    fn installed_state() -> UpdateState {
        let app = PathBuf::from("/Applications/TreeMovieDownloader.app");
        UpdateState::Installed {
            app: app.clone(),
            rollback: crate::updater::AppPlacement::test_for_relaunch(app),
        }
    }

    fn tool(name: &str) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            version: "2026.11.26".to_string(),
            platform: "macos-aarch64".to_string(),
            url: format!("https://example.com/{name}.zip"),
            sha256: "a".repeat(64),
            required: true,
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
    fn formats_library_download_phase() {
        let phase = LibraryInstallPhase::Downloading {
            tool: "yt-dlp".to_string(),
            received: 50,
            total: Some(100),
            step: 1,
            steps: 2,
        };
        assert_eq!(phase.label(), "yt-dlp をダウンロード 50% (1/2)");
        assert_eq!(phase.percent(), Some(50.0));
    }

    #[test]
    fn separates_download_status_from_percentage() {
        let phase = InstallPhase::Downloading {
            received: 50,
            total: Some(100),
        };
        assert_eq!(phase.status_label(), "ダウンロード中");
        assert_eq!(phase.percent(), Some(50.0));

        let phase = LibraryInstallPhase::Downloading {
            tool: "yt-dlp".to_string(),
            received: 50,
            total: Some(100),
            step: 1,
            steps: 2,
        };
        assert_eq!(phase.status_label(), "yt-dlp をダウンロード中 (1/2)");
        assert_eq!(phase.percent(), Some(50.0));
    }

    #[test]
    fn dismisses_available_notifications_without_losing_update_state() {
        let mut controller = UpdateController::new();
        controller.library_check_completed = true;
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());
        controller.library_state = LibraryUpdateState::Available(vec![tool("yt-dlp")]);

        assert!(controller.dismiss_notification_state());
        assert_eq!(controller.available_update_label(), None);
        assert!(controller.has_app_update());
        assert!(controller.has_library_update());
        assert!(matches!(controller.state, UpdateState::Available(_)));
    }

    #[test]
    fn hides_only_the_dismissed_available_update() {
        let mut controller = UpdateController::new();
        controller.library_check_completed = true;
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());
        controller.library_state = LibraryUpdateState::Available(vec![tool("yt-dlp")]);
        controller.app_notification_dismissed = true;

        assert_eq!(
            controller.available_update_label(),
            Some("ライブラリの更新が有ります")
        );
        assert!(controller.has_app_update());
    }

    #[test]
    fn dismissing_library_failure_does_not_reveal_the_app_notification() {
        let mut controller = UpdateController::new();
        controller.library_check_completed = true;
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());
        controller.library_state = LibraryUpdateState::Failed("offline".to_string());

        assert!(controller.dismiss_notification_state());
        assert!(!controller.is_app_update_notification_visible());
        assert!(!controller.is_library_update_notification_visible());
        assert!(controller.has_app_update());
    }

    #[test]
    fn dismissing_library_failure_also_dismisses_installed_app_notification() {
        let mut controller = UpdateController::new();
        controller.state = installed_state();
        controller.library_state = LibraryUpdateState::Failed("offline".to_string());

        assert!(controller.dismiss_notification_state());
        assert!(!controller.is_app_update_notification_visible());
        assert!(!controller.is_library_update_notification_visible());
        assert!(matches!(controller.state, UpdateState::Installed { .. }));
    }

    #[test]
    fn dismissing_failure_does_not_reveal_a_stale_no_update_notification() {
        let mut controller = UpdateController::new();
        controller.state = UpdateState::UpToDate;
        controller.library_state = LibraryUpdateState::Failed("offline".to_string());
        controller.no_update_visible = true;

        assert!(controller.dismiss_notification_state());
        assert!(!controller.no_update_visible);
        assert!(!controller.is_app_update_notification_visible());
        assert!(!controller.is_library_update_notification_visible());
    }

    #[test]
    fn failed_check_hides_notifications_without_losing_update_state() {
        let mut controller = UpdateController::new();
        controller.library_check_completed = true;
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());
        controller.library_state = LibraryUpdateState::Available(vec![tool("yt-dlp")]);
        controller.no_update_visible = true;

        controller.hide_notifications_after_failed_check();

        assert_eq!(controller.available_update_label(), None);
        assert!(!controller.no_update_visible);
        assert!(!controller.is_app_update_notification_visible());
        assert!(!controller.is_library_update_notification_visible());
        assert!(controller.has_app_update());
        assert!(controller.has_library_update());
    }

    #[test]
    fn keeps_notifications_visible_while_installing() {
        let mut controller = UpdateController::new();
        controller.state = UpdateState::Installing(InstallPhase::Verifying);

        assert!(!controller.dismiss_notification_state());
        assert!(controller.is_app_update_notification_visible());
    }

    #[test]
    fn dismisses_the_no_update_notification() {
        let mut controller = UpdateController::new();
        controller.state = UpdateState::UpToDate;
        controller.library_state = LibraryUpdateState::UpToDate;
        controller.no_update_visible = true;

        assert!(controller.dismiss_notification_state());
        assert!(!controller.no_update_visible);
    }

    #[test]
    fn reports_installing_only_for_the_install_state() {
        let mut controller = UpdateController::new();
        assert!(!controller.is_installing());
        controller.state = UpdateState::Installing(InstallPhase::Verifying);
        assert!(controller.is_installing());
    }

    #[test]
    fn installed_update_waits_for_relaunch() {
        let mut controller = UpdateController::new();
        controller.state = installed_state();

        assert!(!controller.is_busy());
        assert!(!controller.is_installing());
        assert!(!controller.has_app_update());
        assert_eq!(controller.install_plan(), InstallPlan::None);
        assert_eq!(controller.state, installed_state());
        assert!(crate::app::APP_UPDATE_COMPLETED_MESSAGE.contains("再起動"));
    }

    #[test]
    fn dismisses_installed_notification_without_losing_relaunch_state() {
        let mut controller = UpdateController::new();
        controller.state = installed_state();

        assert!(controller.dismiss_notification_state());
        assert!(!controller.is_app_update_notification_visible());
        assert!(matches!(controller.state, UpdateState::Installed { .. }));
    }

    #[test]
    fn keeps_the_available_version() {
        let controller = UpdateController {
            state: UpdateState::Available(info()),
            library_state: super::LibraryUpdateState::Idle,
            available: Some(info()),
            available_tools: Vec::new(),
            no_update_visible: false,
            app_notification_dismissed: false,
            library_notification_dismissed: false,
            hide_task: None,
            install_app_after_library: false,
            library_background_started: false,
            library_check_completed: false,
            library_check_unavailable: false,
            dialog_plan: None,
        };
        assert!(controller.has_app_update());
        assert_eq!(
            controller
                .available
                .as_ref()
                .map(|info| info.version.as_str()),
            Some("0.2.0")
        );
        assert!(crate::app::app_update_available_message("0.2.0").contains("0.2.0"));
    }

    #[test]
    fn labels_each_available_update_combination() {
        assert_eq!(
            UpdateController::label_for(true, true),
            Some("アプリとライブラリの更新が有ります")
        );
        assert_eq!(
            UpdateController::label_for(true, false),
            Some("アプリの更新が有ります")
        );
        assert_eq!(
            UpdateController::label_for(false, true),
            Some("ライブラリの更新が有ります")
        );
        assert_eq!(UpdateController::label_for(false, false), None);
    }

    #[test]
    fn plans_libraries_before_the_app_when_both_are_available() {
        let mut controller = UpdateController::new();
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());
        let entry = tool("yt-dlp");
        controller.library_state = LibraryUpdateState::Available(vec![entry.clone()]);

        assert_eq!(controller.install_plan(), InstallPlan::LibrariesThenApp);

        controller.library_state = LibraryUpdateState::UpToDate;
        assert_eq!(controller.install_plan(), InstallPlan::App);

        controller.state = UpdateState::UpToDate;
        controller.library_state = LibraryUpdateState::Available(vec![entry]);
        assert_eq!(controller.install_plan(), InstallPlan::Libraries);

        controller.library_state = LibraryUpdateState::UpToDate;
        assert_eq!(controller.install_plan(), InstallPlan::None);
    }

    #[test]
    fn suppresses_the_update_button_until_both_checks_succeed() {
        let mut controller = UpdateController::new();
        controller.state = UpdateState::Available(info());
        controller.available = Some(info());

        assert_eq!(controller.available_update_label(), None);

        controller.library_check_completed = true;
        assert_eq!(
            controller.available_update_label(),
            Some("アプリの更新が有ります")
        );

        controller.library_state = LibraryUpdateState::CheckFailed("offline".to_string());
        assert_eq!(controller.available_update_label(), None);

        controller.library_check_unavailable = true;
        assert_eq!(
            controller.available_update_label(),
            Some("アプリの更新が有ります")
        );

        controller.library_check_unavailable = false;
        controller.library_state = LibraryUpdateState::UpToDate;
        controller.state = UpdateState::CheckFailed("offline".to_string());
        assert_eq!(controller.available_update_label(), None);
    }

    #[test]
    fn retries_only_the_library_that_failed_after_a_partial_success() {
        let mut controller = UpdateController::new();
        controller.available_tools = vec![tool("yt-dlp"), tool("ffmpeg")];
        controller.library_state =
            LibraryUpdateState::Installing(LibraryInstallPhase::Downloading {
                tool: "ffmpeg".to_string(),
                received: 0,
                total: None,
                step: 2,
                steps: 2,
            });

        controller.remove_completed_library_updates();

        assert_eq!(controller.available_tools.len(), 1);
        assert_eq!(controller.available_tools[0].name, "ffmpeg");
    }

    #[test]
    fn treats_either_update_as_busy() {
        let mut controller = UpdateController::new();
        controller.library_state = LibraryUpdateState::Checking;
        assert!(controller.is_busy());

        controller.library_state = LibraryUpdateState::Idle;
        controller.state = UpdateState::Checking;
        assert!(controller.is_busy());
    }
}
