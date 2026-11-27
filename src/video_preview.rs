//! ダウンロード結果の動画タイトルとサムネイルを表示する共通要素。

use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme as _, StyledExt as _, h_flex, hover_card::HoverCard, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::{QueueItem, display_url};
use crate::thumbnail::Thumbnail;

const HOVER_PREVIEW_SCALE: f32 = 0.5;
const HOVER_PREVIEW_MAX_DIMENSION: f32 = 960.;
const HOVER_PREVIEW_NATIVE_MAX_DIMENSION: f32 = 256.;
const DEFAULT_THUMBNAIL_ASPECT_RATIO: f32 = 16. / 9.;
const MIN_THUMBNAIL_ASPECT_RATIO: f32 = 0.5;
const MAX_THUMBNAIL_ASPECT_RATIO: f32 = 2.0;
const ROW_THUMBNAIL_HEIGHT_REMS: f32 = 2.25;
const CARD_THUMBNAIL_HEIGHT_REMS: f32 = 4.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewSize {
    Row,
    Card,
}

#[derive(IntoElement)]
pub(crate) struct VideoPreview {
    item_id: u64,
    title: SharedString,
    url: SharedString,
    thumbnail: Option<Thumbnail>,
    size: PreviewSize,
    show_url: bool,
}

impl VideoPreview {
    pub(crate) fn from_item(item: &QueueItem, size: PreviewSize) -> Option<Self> {
        let title = item.title().cloned();
        let thumbnail = item.thumbnail().cloned();
        if title.is_none() && thumbnail.is_none() {
            return None;
        }
        let show_url = size == PreviewSize::Row && title.is_some();
        Some(Self {
            item_id: item.id(),
            title: title.unwrap_or_else(|| item.url().clone()),
            url: item.url().clone(),
            thumbnail,
            size,
            show_url,
        })
    }

    fn thumbnail_box(&self, cx: &App) -> AnyElement {
        let (box_width, box_height) =
            thumbnail_box_size(self.size, self.thumbnail.as_ref().map(Thumbnail::size));
        let placeholder = div()
            .w(rems(box_width))
            .h(rems(box_height))
            .flex_shrink_0()
            .rounded_sm()
            .bg(cx.theme().muted);

        let Some(thumbnail) = &self.thumbnail else {
            return placeholder.into_any_element();
        };
        let preview = thumbnail.preview();
        let full = thumbnail.full();
        let (width, height) = thumbnail.size();
        let (hover_width, hover_height) = hover_preview_size(width, height);
        let item_id = self.item_id;
        let hover_id = match self.size {
            PreviewSize::Row => "video-thumbnail-hover-row",
            PreviewSize::Card => "video-thumbnail-hover-card",
        };
        let preview_id = match self.size {
            PreviewSize::Row => "video-thumbnail-row",
            PreviewSize::Card => "video-thumbnail-card",
        };
        let full_id = match self.size {
            PreviewSize::Row => "video-thumbnail-full-row",
            PreviewSize::Card => "video-thumbnail-full-card",
        };

        HoverCard::new((hover_id, item_id))
            .flex_shrink_0()
            .open_delay(Duration::from_millis(500))
            .close_delay(Duration::from_millis(200))
            .trigger(
                placeholder.child(
                    img(preview)
                        .id((preview_id, item_id))
                        .size_full()
                        .rounded_sm()
                        .object_fit(ObjectFit::Contain),
                ),
            )
            .content(move |_, _, _| {
                img(full.clone())
                    .id((full_id, item_id))
                    .w(px(hover_width))
                    .h(px(hover_height))
                    .rounded_sm()
                    .object_fit(ObjectFit::Contain)
            })
            .into_any_element()
    }
}

fn thumbnail_box_size(size: PreviewSize, image_size: Option<(u32, u32)>) -> (f32, f32) {
    let height = match size {
        PreviewSize::Row => ROW_THUMBNAIL_HEIGHT_REMS,
        PreviewSize::Card => CARD_THUMBNAIL_HEIGHT_REMS,
    };
    let aspect_ratio = image_size
        .map(|(width, height)| width as f32 / height.max(1) as f32)
        .unwrap_or(DEFAULT_THUMBNAIL_ASPECT_RATIO)
        .clamp(MIN_THUMBNAIL_ASPECT_RATIO, MAX_THUMBNAIL_ASPECT_RATIO);
    (height * aspect_ratio, height)
}

fn hover_preview_size(width: u32, height: u32) -> (f32, f32) {
    let width = width as f32;
    let height = height as f32;
    if width <= HOVER_PREVIEW_NATIVE_MAX_DIMENSION && height <= HOVER_PREVIEW_NATIVE_MAX_DIMENSION {
        return (width, height);
    }
    let scale = HOVER_PREVIEW_SCALE
        .min(HOVER_PREVIEW_MAX_DIMENSION / width.max(1.))
        .min(HOVER_PREVIEW_MAX_DIMENSION / height.max(1.));
    (width * scale, height * scale)
}

impl RenderOnce for VideoPreview {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let thumbnail = self.thumbnail_box(cx);
        let title = div()
            .text_xs()
            .font_medium()
            .text_color(cx.theme().foreground)
            .truncate()
            .child(self.title)
            .when(self.size == PreviewSize::Card, |this| this.text_sm());
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(title)
            .when(self.show_url, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .font_family(cx.theme().mono_font_family.clone())
                        .truncate()
                        .child(display_url(self.url.as_ref())),
                )
            });

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(thumbnail)
            .child(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CARD_THUMBNAIL_HEIGHT_REMS, DEFAULT_THUMBNAIL_ASPECT_RATIO, HOVER_PREVIEW_MAX_DIMENSION,
        MAX_THUMBNAIL_ASPECT_RATIO, MIN_THUMBNAIL_ASPECT_RATIO, PreviewSize,
        ROW_THUMBNAIL_HEIGHT_REMS, hover_preview_size, thumbnail_box_size,
    };

    #[test]
    fn vertical_thumbnails_keep_the_height_and_narrow_the_width() {
        let (width, height) = thumbnail_box_size(PreviewSize::Row, Some((1080, 1920)));
        assert_eq!(height, ROW_THUMBNAIL_HEIGHT_REMS);
        assert!((width / height - 9. / 16.).abs() < f32::EPSILON);

        let (width, height) = thumbnail_box_size(PreviewSize::Card, Some((1080, 1920)));
        assert_eq!(height, CARD_THUMBNAIL_HEIGHT_REMS);
        assert!((width / height - 9. / 16.).abs() < f32::EPSILON);
    }

    #[test]
    fn landscape_and_missing_thumbnails_keep_their_aspect_ratio() {
        let (width, height) = thumbnail_box_size(PreviewSize::Row, Some((1920, 1080)));
        assert_eq!(height, ROW_THUMBNAIL_HEIGHT_REMS);
        assert!((width / height - 16. / 9.).abs() < f32::EPSILON);

        let (width, height) = thumbnail_box_size(PreviewSize::Card, None);
        assert_eq!(height, CARD_THUMBNAIL_HEIGHT_REMS);
        assert!((width / height - DEFAULT_THUMBNAIL_ASPECT_RATIO).abs() < f32::EPSILON);
    }

    #[test]
    fn extreme_aspect_ratios_are_clamped_to_the_available_lane() {
        let (width, height) = thumbnail_box_size(PreviewSize::Row, Some((4096, 480)));
        assert!((width / height - MAX_THUMBNAIL_ASPECT_RATIO).abs() < f32::EPSILON);

        let (width, height) = thumbnail_box_size(PreviewSize::Card, Some((480, 4096)));
        assert!((width / height - MIN_THUMBNAIL_ASPECT_RATIO).abs() < f32::EPSILON);
    }

    #[test]
    fn hover_previews_are_half_size_unless_the_maximum_is_exceeded() {
        assert_eq!(hover_preview_size(120, 90), (120., 90.));
        assert_eq!(hover_preview_size(1280, 720), (640., 360.));
        let (width, height) = hover_preview_size(4096, 2048);
        assert_eq!(width, HOVER_PREVIEW_MAX_DIMENSION);
        assert_eq!(height, HOVER_PREVIEW_MAX_DIMENSION / 2.);
    }
}
