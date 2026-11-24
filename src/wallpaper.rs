//! アプリ全体の背景画像の状態と表示。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

use gpui_kit::*;

use crate::app_settings::WallpaperSource;

#[derive(Clone, Debug)]
pub struct WallpaperState {
    pub source: Option<WallpaperSource>,
    pub opacity: f32,
}

/// 背景画像の要素を作る。
///
/// GPUIの`img`は画像のアスペクト比をレイアウトへ反映するため、要素の高さが
/// 親を超えてしまうことがある。ここでは親の領域をそのまま使い、描画時に
/// `ObjectFit`で画像を収める。
pub fn wallpaper_image(
    cx: &App,
    source: &WallpaperSource,
    fit: ObjectFit,
    opacity: f32,
) -> Option<AnyElement> {
    let image = wallpaper_source_image(cx, source)?;

    Some(
        wallpaper_layer(image, fit)
            .opacity(opacity.clamp(0.0, 1.0))
            .into_any_element(),
    )
}

/// 背景画像の要素を、親の領域いっぱいに描画する。
pub fn wallpaper_layer(image: Arc<RenderImage>, fit: ObjectFit) -> Div {
    div().size_full().child(WallpaperImage { image, fit })
}

/// 背景画像を読み込む。JXLはデコードしてキャッシュする。
pub fn wallpaper_source_image(cx: &App, source: &WallpaperSource) -> Option<Arc<RenderImage>> {
    load_wallpaper(cx, source)
}

struct WallpaperImage {
    image: Arc<RenderImage>,
    fit: ObjectFit,
}

impl Element for WallpaperImage {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let image_bounds = self.fit.get_bounds(bounds, self.image.size(0));
        let _ = window.paint_image(
            bounds,
            image_bounds,
            Corners::default(),
            self.image.clone(),
            0,
            false,
        );
    }
}

impl IntoElement for WallpaperImage {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[derive(Clone, PartialEq, Eq)]
enum WallpaperKey {
    Bundled(String),
    User(PathBuf, Option<Duration>),
}

type WallpaperCacheEntry = (WallpaperKey, Arc<RenderImage>);

fn wallpaper_cache() -> &'static Mutex<Option<WallpaperCacheEntry>> {
    static CACHE: OnceLock<Mutex<Option<WallpaperCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn load_wallpaper(cx: &App, source: &WallpaperSource) -> Option<Arc<RenderImage>> {
    let key = match source {
        WallpaperSource::Bundled(name) => WallpaperKey::Bundled(name.clone()),
        WallpaperSource::User(path) => WallpaperKey::User(path.clone(), modified_at(path)),
    };

    if let Ok(cache) = wallpaper_cache().lock()
        && let Some((cached_key, image)) = cache.as_ref()
        && *cached_key == key
    {
        return Some(image.clone());
    }

    let image = match source {
        WallpaperSource::Bundled(name) => {
            let path = format!("images/wallpaper/{name}");
            let bytes = cx.asset_source().load(&path).ok().flatten()?;
            decode_standard_image(&bytes)?
        }
        WallpaperSource::User(path) if is_jxl(path) => decode_jxl(path)?,
        WallpaperSource::User(path) => decode_standard_image(&std::fs::read(path).ok()?)?,
    };

    if let Ok(mut cache) = wallpaper_cache().lock() {
        *cache = Some((key, image.clone()));
    }
    Some(image)
}

fn modified_at(path: &Path) -> Option<Duration> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
}

fn is_jxl(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jxl"))
}

fn decode_standard_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(to_render_image(image))
}

fn decode_jxl(path: &Path) -> Option<Arc<RenderImage>> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = jxl_oxide::integration::JxlDecoder::new(file).ok()?;
    let image = image::DynamicImage::from_decoder(decoder).ok()?;
    Some(to_render_image(image))
}

fn to_render_image(image: image::DynamicImage) -> Arc<RenderImage> {
    let rgba = rgba_to_bgra(image.to_rgba8());
    Arc::new(RenderImage::new(vec![image::Frame::new(rgba)]))
}

/// GPUIの画像バッファはBGRA順のため、RGBAから変換する。
fn rgba_to_bgra(mut rgba: image::RgbaImage) -> image::RgbaImage {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::{is_jxl, rgba_to_bgra};
    use std::path::Path;

    #[test]
    fn detects_jxl_extension_case_insensitively() {
        assert!(is_jxl(Path::new("wallpaper.jxl")));
        assert!(is_jxl(Path::new("wallpaper.JXL")));
        assert!(!is_jxl(Path::new("wallpaper.jpg")));
        assert!(!is_jxl(Path::new("wallpaper")));
    }

    #[test]
    fn converts_rgba_to_bgra_for_gpui() {
        use image::{Rgba, RgbaImage};

        let rgba = RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 40]));
        let bgra = rgba_to_bgra(rgba);

        assert_eq!(bgra.get_pixel(0, 0).0, [30, 20, 10, 40]);
    }
}
