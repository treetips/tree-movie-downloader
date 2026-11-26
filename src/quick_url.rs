//! かんたんダウンロードで受け付ける動画URLの検証。

use crate::model::is_http_url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuickUrlError {
    Empty,
    Invalid,
    Multiple,
    Collection,
}

impl QuickUrlError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Empty => "動画URLを入力してください。",
            Self::Invalid => "http:// または https:// で始まる正しいURLを入力してください。",
            Self::Multiple => "URLは1件だけ入力してください。",
            Self::Collection => {
                "この画面ではプレイリスト・チャンネルをダウンロードできません。一括ダウンロードを使ってください。"
            }
        }
    }
}

pub(crate) fn validate_quick_url(input: &str) -> Result<String, QuickUrlError> {
    let url = input.trim();
    if url.is_empty() {
        return Err(QuickUrlError::Empty);
    }
    if url.lines().count() > 1 {
        return Err(QuickUrlError::Multiple);
    }
    if !is_http_url(url) {
        return Err(QuickUrlError::Invalid);
    }
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    if is_youtube_collection(rest) {
        return Err(QuickUrlError::Collection);
    }

    Ok(url.to_string())
}

fn is_youtube_collection(rest: &str) -> bool {
    let (authority, path_and_query) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host != "youtube.com" && !host.ends_with(".youtube.com") {
        return false;
    }

    let path = path_and_query.split(['?', '#']).next().unwrap_or_default();
    if path == "playlist" || path.starts_with("playlist/") {
        return true;
    }
    path.is_empty()
        || path.starts_with('@')
        || path.starts_with("channel/")
        || path.starts_with("user/")
        || path.starts_with("c/")
        || path.starts_with("feed/")
        || path == "results"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_single_video_urls() {
        for url in [
            "https://www.youtube.com/watch?v=abc123&list=PL1",
            "https://youtu.be/abc123",
            "https://www.youtube.com/shorts/abc123",
            "https://www.youtube.com/clip/Ugkx123",
            "https://www.youtube.com/v/abc123",
            "https://www.youtube.com/watch_popup?v=abc123",
            "https://x.com/example/status/123",
            "https://www.twitch.tv/videos/123",
            "HTTPS://example.com/video",
        ] {
            assert_eq!(validate_quick_url(url).as_deref(), Ok(url));
        }
    }

    #[test]
    fn rejects_empty_invalid_and_multiple_urls() {
        assert_eq!(validate_quick_url("  "), Err(QuickUrlError::Empty));
        for url in [
            "example.com/video",
            "ftp://example.com/video",
            "https://",
            "https://?video=abc",
            "https://example.com/a b",
            "https://example.com/a\\b",
        ] {
            assert_eq!(
                validate_quick_url(url),
                Err(QuickUrlError::Invalid),
                "{url}"
            );
        }
        assert_eq!(
            validate_quick_url("https://example.com/a\nhttps://example.com/b"),
            Err(QuickUrlError::Multiple)
        );
    }

    #[test]
    fn rejects_youtube_collections() {
        for url in [
            "https://www.youtube.com/playlist?list=PL1",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/@channel/videos",
            "https://www.youtube.com/channel/UC123",
            "https://www.youtube.com/c/channel",
            "https://www.youtube.com/results?search_query=video",
            "https://www.youtube.com./playlist?list=PL1",
            "https://www.youtube.com/",
        ] {
            assert_eq!(
                validate_quick_url(url),
                Err(QuickUrlError::Collection),
                "{url}"
            );
        }
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            validate_quick_url("  https://example.com/video\n").as_deref(),
            Ok("https://example.com/video")
        );
    }
}
