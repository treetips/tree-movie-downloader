//! Tree Movie Downloader — yt-dlp を GUI で扱うデスクトップアプリ。

mod activity_log;
mod app;
mod app_settings;
mod downloader;
mod model;
mod queue_table;
mod settings_page;
mod tool_bootstrap;
mod tools;
mod update_controller;
mod updater;
mod wallpaper;

use std::borrow::Cow;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use app::{AddUrls, AppView, CheckForUpdates, QuitApp, RetryFailed, StartOrPause};

const ICON_PNG: &[u8] = include_bytes!("../assets/images/icon-tree-01.png");

/// アプリのアセット。GPUI Kitのアイコンカタログに、アプリアイコンを追加する。
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            "images/icon-tree-01.png" => {
                return Ok(Some(Cow::Borrowed(ICON_PNG)));
            }
            "images/wallpaper/wallpaper1.jpg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../assets/images/wallpaper/wallpaper1.jpg"
                ))));
            }
            "images/wallpaper/wallpaper2.jpg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../assets/images/wallpaper/wallpaper2.jpg"
                ))));
            }
            "images/wallpaper/wallpaper3.jpg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../assets/images/wallpaper/wallpaper3.jpg"
                ))));
            }
            _ => {}
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::AllAssets.list(path)?;
        for asset in [
            "images/icon-tree-01.png",
            "images/wallpaper/wallpaper1.jpg",
            "images/wallpaper/wallpaper2.jpg",
            "images/wallpaper/wallpaper3.jpg",
        ] {
            if asset.starts_with(path) {
                paths.push(asset.into());
            }
        }
        Ok(paths)
    }
}

#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let bytes = ICON_PNG;
    // SAFETY: `bytes` is a static slice valid for `bytes.len()` bytes, and
    // `initWithBytes:length:` copies that data into the retained `NSData`.
    let data = unsafe {
        NSData::initWithBytes_length(
            NSData::alloc(),
            bytes.as_ptr().cast::<std::ffi::c_void>(),
            bytes.len(),
        )
    };
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let application = NSApplication::sharedApplication(main_thread);
    // SAFETY: AppKit is accessed on the main thread with a valid retained image.
    unsafe {
        application.setApplicationIconImage(Some(&icon));
    }
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {}

fn main() {
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        if cfg!(debug_assertions) {
            set_dock_icon();
        }

        cx.on_action(|_: &CheckForUpdates, cx| {
            let Some(window) = cx.active_window() else {
                return;
            };
            let _ = window.update(cx, |root, window, cx| {
                let Ok(view) = root.downcast::<AppView>() else {
                    return;
                };
                view.update(cx, |view, cx| view.check_for_updates(window, cx));
            });
        });
        cx.on_action(|_: &QuitApp, cx| cx.quit());

        cx.bind_keys([
            KeyBinding::new("cmd-n", AddUrls, None),
            KeyBinding::new("cmd-enter", StartOrPause, None),
            KeyBinding::new("cmd-shift-r", RetryFailed, None),
            KeyBinding::new("cmd-q", QuitApp, None),
        ]);

        cx.set_menus([Menu::new("Tree Movie Downloader").items([
            MenuItem::action("アップデートを確認", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("終了", QuitApp),
        ])]);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(120.), px(90.)),
                size(px(1280.), px(820.)),
            ))),
            window_min_size: Some(size(px(1120.), px(700.))),
            ..TitleBar::window_options()
        };

        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AppView::new(window, cx))
        })
        .expect("failed to open window");
    });
}
