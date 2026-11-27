//! 動画サムネイルの取得と表示用画像への正規化。

use std::io::{Cursor, Read};
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui_kit::{Image, ImageFormat};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};

const MAX_THUMBNAIL_BYTES: u64 = 5 * 1024 * 1024;
const MAX_SOURCE_DIMENSION: u32 = 4096;
const MAX_SOURCE_ALLOCATION: u64 = 64 * 1024 * 1024;
const THUMBNAIL_MAX_DIMENSION: u32 = 256;

/// 一覧表示用の縮小画像と、ホバー時に原寸で表示する元画像を保持する。
#[derive(Clone)]
pub(crate) struct Thumbnail {
    preview: Arc<Image>,
    full: Arc<Image>,
    width: u32,
    height: u32,
}

impl Thumbnail {
    pub(crate) fn new(preview: Arc<Image>, full: Arc<Image>, width: u32, height: u32) -> Self {
        Self {
            preview,
            full,
            width,
            height,
        }
    }

    pub(crate) fn preview(&self) -> Arc<Image> {
        self.preview.clone()
    }

    pub(crate) fn full(&self) -> Arc<Image> {
        self.full.clone()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// サムネイルを取得し、一覧用の縮小画像と元画像を用意する。
pub(crate) fn load_thumbnail(url: &str) -> Result<Thumbnail, String> {
    validate_thumbnail_url(url)?;
    let response = thumbnail_agent()
        .get(url)
        .call()
        .map_err(|error| format!("サムネイルを取得できませんでした: {error}"))?;
    if (300..400).contains(&response.status().as_u16()) {
        return Err("サムネイルURLがリダイレクトを返すため取得できません。".into());
    }
    if response
        .body()
        .content_length()
        .is_some_and(|length| length > MAX_THUMBNAIL_BYTES)
    {
        return Err("サムネイルのサイズが5MBを超えています。".into());
    }

    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_THUMBNAIL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("サムネイルを読み込めませんでした: {error}"))?;
    thumbnail_from_bytes(bytes)
}

fn thumbnail_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .build();
        ureq::Agent::with_parts(config, (), PublicResolver)
    })
}

#[derive(Debug)]
struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let resolver = DefaultResolver::default();
        let addresses = resolver.resolve(uri, config, timeout)?;
        let mut public_addresses = self.empty();
        for address in &addresses {
            if is_public_address(address.ip()) {
                public_addresses.push(*address);
            }
        }
        if public_addresses.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(public_addresses)
        }
    }
}

fn validate_thumbnail_url(url: &str) -> Result<(), String> {
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|error| format!("サムネイルURLが不正です: {error}"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err("サムネイルURLはhttpまたはhttpsである必要があります。".into());
    }
    let host = uri
        .host()
        .ok_or_else(|| "サムネイルURLにホストがありません。".to_string())?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err("ローカルホストのサムネイルURLは取得できません。".into());
    }

    if let Ok(ip) = host.parse::<IpAddr>()
        && !is_public_address(ip)
    {
        return Err("プライベートネットワークのサムネイルURLは取得できません。".into());
    }
    Ok(())
}

fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_multicast()
                || address.is_broadcast()
                || octets[0] == 0
                || octets[0] >= 240
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
                || (octets[0] == 198 && octets[1] == 18)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
        }
        IpAddr::V6(address) => {
            if let Some(address) = address.to_ipv4_mapped() {
                return is_public_address(IpAddr::V4(address));
            }
            let segments = address.segments();
            let ipv4_compatible = segments[..6].iter().all(|segment| *segment == 0);
            !(address.is_loopback()
                || address.is_unicast_link_local()
                || address.is_unspecified()
                || address.is_multicast()
                || address.is_unique_local()
                || ipv4_compatible
                || (segments[0] == 0x0100 && segments[1] == 0)
                || (segments[0] == 0x0064 && segments[1] == 0xff9b)
                || (segments[0] == 0x2002)
                || (segments[0] == 0x2001 && segments[1] == 0)
                || (segments[0] == 0x2001 && segments[1] == 0x0002)
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

fn thumbnail_from_bytes(bytes: Vec<u8>) -> Result<Thumbnail, String> {
    if bytes.len() as u64 > MAX_THUMBNAIL_BYTES {
        return Err("サムネイルのサイズが5MBを超えています。".into());
    }
    let source_format = image::guess_format(&bytes)
        .map_err(|error| format!("サムネイルの形式を判定できません: {error}"))?;
    let full_format = gpui_image_format(source_format)
        .ok_or_else(|| "サムネイルの画像形式に対応していません。".to_string())?;

    let mut reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|error| format!("サムネイルを読み込めません: {error}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_SOURCE_ALLOCATION);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|error| format!("サムネイルのデコーダーを作成できません: {error}"))?;
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("サムネイルをデコードできません: {error}"))?;
    let (width, height) = (image.width(), image.height());
    let resized =
        if image.width() > THUMBNAIL_MAX_DIMENSION || image.height() > THUMBNAIL_MAX_DIMENSION {
            image.thumbnail(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION)
        } else {
            image
        };

    let mut png = Cursor::new(Vec::new());
    resized
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("サムネイルをPNGへ変換できません: {error}"))?;
    let preview = Arc::new(Image::from_bytes(ImageFormat::Png, png.into_inner()));
    let full = Arc::new(Image::from_bytes(full_format, bytes));
    Ok(Thumbnail::new(preview, full, width, height))
}

fn gpui_image_format(format: image::ImageFormat) -> Option<ImageFormat> {
    match format {
        image::ImageFormat::Png => Some(ImageFormat::Png),
        image::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        image::ImageFormat::WebP => Some(ImageFormat::Webp),
        image::ImageFormat::Gif => Some(ImageFormat::Gif),
        image::ImageFormat::Bmp => Some(ImageFormat::Bmp),
        image::ImageFormat::Tiff => Some(ImageFormat::Tiff),
        image::ImageFormat::Ico => Some(ImageFormat::Ico),
        image::ImageFormat::Pnm => Some(ImageFormat::Pnm),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_original_image_and_a_bounded_preview() {
        let source = image::RgbaImage::from_pixel(800, 450, image::Rgba([10, 20, 30, 255]));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode source thumbnail");

        let thumbnail = thumbnail_from_bytes(png.into_inner()).expect("normalize thumbnail");
        assert_eq!(thumbnail.size(), (800, 450));
        assert_eq!(thumbnail.preview().format, ImageFormat::Png);
        assert_eq!(thumbnail.full().format, ImageFormat::Png);

        let preview = image::load_from_memory(&thumbnail.preview().bytes).expect("decode preview");
        assert_eq!(preview.width(), 256);
        assert_eq!(preview.height(), 144);
        let full = image::load_from_memory(&thumbnail.full().bytes).expect("decode full image");
        assert_eq!(full.width(), 800);
        assert_eq!(full.height(), 450);
    }

    #[test]
    fn small_thumbnails_are_not_upscaled() {
        let source = image::RgbaImage::from_pixel(120, 90, image::Rgba([10, 20, 30, 255]));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode source thumbnail");

        let thumbnail = thumbnail_from_bytes(png.into_inner()).expect("normalize thumbnail");
        let preview = image::load_from_memory(&thumbnail.preview().bytes).expect("decode preview");
        assert_eq!(preview.width(), 120);
        assert_eq!(preview.height(), 90);
    }

    #[test]
    fn rejects_invalid_and_oversized_thumbnails() {
        assert!(thumbnail_from_bytes(b"not an image".to_vec()).is_err());
        assert!(thumbnail_from_bytes(vec![0; MAX_THUMBNAIL_BYTES as usize + 1]).is_err());
    }

    #[test]
    fn rejects_non_web_and_private_thumbnail_urls() {
        assert!(validate_thumbnail_url("file:///tmp/image.png").is_err());
        assert!(validate_thumbnail_url("http://127.0.0.1/image.png").is_err());
        assert!(validate_thumbnail_url("http://[::1]/image.png").is_err());
        assert!(validate_thumbnail_url("http://100.64.0.1/image.png").is_err());
        assert!(validate_thumbnail_url("http://[64:ff9b::7f00:1]/image.png").is_err());
        assert!(validate_thumbnail_url("https://localhost/image.png").is_err());
        assert!(validate_thumbnail_url("https://8.8.8.8/image.png").is_ok());
    }
}
