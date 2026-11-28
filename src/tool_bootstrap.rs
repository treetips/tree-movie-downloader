//! 起動時の外部ツール準備状態。
//!
//! ダウンロード処理そのものは [`crate::tools`] が担い、ここでは進捗を GPUI の
//! 状態へ変換して画面へ通知する。準備が終わるまでメイン画面は表示しない。

use gpui_kit::Context;

use crate::tools::{self, BootstrapEvent, ToolPaths};

/// 起動時のツール準備フェーズ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapPhase {
    /// 配布情報を確認している。
    Checking,
    /// ツールをダウンロードしている。
    Downloading {
        tool: String,
        received: u64,
        total: Option<u64>,
        step: usize,
        steps: usize,
    },
    /// ダウンロードしたツールを配置している。
    Installing {
        tool: String,
        step: usize,
        steps: usize,
    },
    /// すべてのツールが揃った。
    Ready,
    /// 準備に失敗した。再試行できる。
    Failed(String),
}

impl From<BootstrapEvent> for BootstrapPhase {
    fn from(event: BootstrapEvent) -> Self {
        match event {
            BootstrapEvent::Checking => Self::Checking,
            BootstrapEvent::Downloading {
                tool,
                received,
                total,
                step,
                steps,
            } => Self::Downloading {
                tool,
                received,
                total,
                step,
                steps,
            },
            BootstrapEvent::Installing { tool, step, steps } => {
                Self::Installing { tool, step, steps }
            }
            BootstrapEvent::Ready => Self::Ready,
            BootstrapEvent::Failed(message) => Self::Failed(message),
        }
    }
}

/// 起動時のツール準備を管理する。
pub struct ToolBootstrap {
    phase: BootstrapPhase,
    paths: ToolPaths,
    started: bool,
}

impl ToolBootstrap {
    pub fn new() -> Self {
        Self {
            phase: BootstrapPhase::Checking,
            paths: ToolPaths::from_env(),
            started: false,
        }
    }

    /// 準備を開始する。既にツールが揃っていれば通信せずに完了する。
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.started {
            return;
        }
        self.started = true;

        if self.paths.is_complete() && !self.paths.has_legacy_tools() {
            self.phase = BootstrapPhase::Ready;
            cx.notify();
            return;
        }

        self.phase = BootstrapPhase::Checking;
        cx.notify();

        let (sender, receiver) = smol::channel::unbounded();
        let paths = self.paths.clone();
        std::thread::spawn(move || tools::bootstrap(paths, sender));

        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let finished = matches!(event, BootstrapEvent::Ready | BootstrapEvent::Failed(_));
                let phase = BootstrapPhase::from(event);
                if this
                    .update(cx, |state, cx| {
                        state.phase = phase;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    /// 失敗後に最初からやり直す。
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.phase, BootstrapPhase::Failed(_)) {
            return;
        }
        self.started = false;
        self.phase = BootstrapPhase::Checking;
        self.start(cx);
    }

    pub fn phase(&self) -> &BootstrapPhase {
        &self.phase
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.phase, BootstrapPhase::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_download_events_to_phases() {
        let phase = BootstrapPhase::from(BootstrapEvent::Downloading {
            tool: "yt-dlp".to_string(),
            received: 10,
            total: Some(100),
            step: 1,
            steps: 2,
        });
        assert_eq!(
            phase,
            BootstrapPhase::Downloading {
                tool: "yt-dlp".to_string(),
                received: 10,
                total: Some(100),
                step: 1,
                steps: 2,
            }
        );
    }

    #[test]
    fn reports_ready_only_after_the_ready_event() {
        let phase = BootstrapPhase::from(BootstrapEvent::Ready);
        assert!(matches!(phase, BootstrapPhase::Ready));
    }
}
