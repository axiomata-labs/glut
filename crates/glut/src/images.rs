//! Images sent to a model (`docs/plans/glut-tui.md`, T45): the owner's (pasted, dropped, `glut run --image`) and
//! those `read_file` gives the model. Each is scaled to what the APIs take — at most [`LONG_SIDE`] pixels on its long
//! side — and kept once, by its content's hash, in `~/.glut/images`: a message holds a reference ([`Image`]), never
//! the data, and the model client reads the file as it sends ([`base64_of`]).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The folder below `~/.glut`.
pub const DIR: &str = "images";
/// The longest side sent: what Anthropic scales a larger image down to anyway, after counting it whole.
pub const LONG_SIDE: u32 = 1568;
/// A PNG larger than this, scaled, goes as JPEG: the APIs take at most 5 MB of an image, and its base64 a third more.
const MOST_PNG_BYTES: usize = 3_500_000;
/// The quality of a JPEG made of a PNG too large.
const JPEG_QUALITY: u8 = 85;
/// The largest image file read in: a screenshot is a few MB, nothing a model is shown needs more.
pub const MOST_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// The widest and highest image decoded: the APIs take at most 8000 px a side. Without it a JPEG's decoder would
/// take any size its header claims.
const MOST_DECODED_SIDE: u32 = 8192;
/// What decoding one image may take: one 8192 × 8192 picture of four bytes a pixel.
const MOST_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// An image a message carries, by reference: its number in the session, where it is kept, its type and size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    /// `#1`, `#2` … through the session, so "image 2" names one image.
    pub number: u32,
    /// The kept file, in `~/.glut/images`.
    pub path: PathBuf,
    /// `image/png` or `image/jpeg`.
    pub media: String,
    pub width: u32,
    pub height: u32,
    /// The kept file's size.
    pub bytes: u64,
}

impl Image {
    /// `[image #1 · 1280×800 · 240 kB]`: how the image stands in a message's text and in the stream.
    pub fn label(&self) -> String {
        format!(
            "[image #{} · {}×{} · {}]",
            self.number,
            self.width,
            self.height,
            size(self.bytes)
        )
    }
}

/// `240 kB`, `1.4 MB`.
fn size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} kB", bytes.div_ceil(1000))
    }
}

/// Where the images are kept, below the owner's `home`.
pub fn dir(home: &Path) -> PathBuf {
    home.join(crate::providers::DIR).join(DIR)
}

/// Whether `bytes` begin as an image Glut reads (PNG, JPEG, GIF, WebP).
pub fn is_image(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok_and(|format| {
        matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        )
    })
}

/// The image in `bytes` kept as image `number`: scaled to [`LONG_SIDE`], written as PNG (as JPEG where a PNG would be
/// too large) under its hash — once, however often it comes.
///
/// # Errors
///
/// A message when the bytes are no image Glut reads or the file cannot be written.
pub fn keep(home: &Path, bytes: &[u8], number: u32) -> Result<Image, String> {
    if !is_image(bytes) {
        return Err("no image Glut reads (PNG, JPEG, GIF, WebP)".into());
    }
    if bytes.len() as u64 > MOST_FILE_BYTES {
        return Err(format!(
            "the image is larger than {}",
            size(MOST_FILE_BYTES)
        ));
    }
    // A decoder that panics on a crafted file (a model may read anything in the project) fails this image, never
    // the session.
    let (data, media, extension, (width, height)) = std::panic::catch_unwind(|| scaled(bytes))
        .unwrap_or_else(|_| Err("the image cannot be read: its decoder gave up on it".into()))?;
    let hash: String = Sha256::digest(&data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let dir = dir(home);
    let path = dir.join(format!("{hash}.{extension}"));
    if !path.exists() {
        crate::providers::make_private_dir(&dir)
            .map_err(|err| format!("{}: {err}", dir.display()))?;
        crate::providers::write_replacing(&dir, &path, &data, 0o600)
            .map_err(|err| format!("{}: {err}", path.display()))?;
    } else {
        // Kept again: young again, so `glut clean`'s grace (T82) covers a draft or a session not saved yet.
        let _ = std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    }
    Ok(Image {
        number,
        path,
        media: media.to_owned(),
        width,
        height,
        bytes: data.len() as u64,
    })
}

/// What an image is kept as: its data, type, file extension and size in pixels.
type Encoded = (Vec<u8>, &'static str, &'static str, (u32, u32));

/// `bytes` decoded within [`MOST_DECODED_SIDE`] and [`MOST_DECODED_BYTES`], scaled to [`LONG_SIDE`] and encoded.
fn scaled(bytes: &[u8]) -> Result<Encoded, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_DECODED_SIDE);
    limits.max_image_height = Some(MOST_DECODED_SIDE);
    limits.max_alloc = Some(MOST_DECODED_BYTES);
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|err| format!("the image cannot be read: {err}"))?;
    let scaled = if decoded.width().max(decoded.height()) > LONG_SIDE {
        decoded.resize(LONG_SIDE, LONG_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        decoded
    };
    let size = (scaled.width(), scaled.height());
    let mut png = Vec::new();
    scaled
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    if png.len() <= MOST_PNG_BYTES {
        return Ok((png, "image/png", "png", size));
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), JPEG_QUALITY)
        .encode_image(&scaled.to_rgb8())
        .map_err(|err| err.to_string())?;
    Ok((jpeg, "image/jpeg", "jpg", size))
}

/// The kept image `image` as `columns` × `rows` pixels of RGB, row by row, for a picture in a terminal (T46): drawn
/// over black where it is transparent. Only a file Glut keeps is read.
///
/// # Errors
///
/// A message when it is no kept image, is gone, or cannot be read.
pub fn thumbnail(image: &Image, columns: u32, rows: u32) -> Result<Vec<Vec<[u8; 3]>>, String> {
    kept_media(&image.path)
        .ok_or_else(|| format!("image #{} is no image Glut keeps", image.number))?;
    let bytes = read_file(&image.path).map_err(|_| format!("image #{} is gone", image.number))?;
    let columns = columns.max(1);
    let rows = rows.max(1);
    std::panic::catch_unwind(|| {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MOST_DECODED_SIDE);
        limits.max_image_height = Some(MOST_DECODED_SIDE);
        limits.max_alloc = Some(MOST_DECODED_BYTES);
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|err| err.to_string())?;
        reader.limits(limits);
        let decoded = reader.decode().map_err(|err| err.to_string())?;
        let small = decoded
            .resize_exact(columns, rows, image::imageops::FilterType::Triangle)
            .to_rgba8();
        Ok(small
            .rows()
            .map(|row| {
                row.map(|pixel| {
                    let [r, g, b, a] = pixel.0;
                    let over = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
                    [over(r), over(g), over(b)]
                })
                .collect()
            })
            .collect())
    })
    .unwrap_or_else(|_| Err("its decoder gave up on it".into()))
}

/// The kept image `image` as a PNG within `width` × `height` pixels, its proportions kept — what a terminal that
/// draws images is sent (T46b). Only a file Glut keeps is read.
///
/// # Errors
///
/// A message when it is no kept image, is gone, or cannot be read.
pub fn png_within(image: &Image, width: u32, height: u32) -> Result<Vec<u8>, String> {
    kept_media(&image.path)
        .ok_or_else(|| format!("image #{} is no image Glut keeps", image.number))?;
    let bytes = read_file(&image.path).map_err(|_| format!("image #{} is gone", image.number))?;
    std::panic::catch_unwind(|| {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MOST_DECODED_SIDE);
        limits.max_image_height = Some(MOST_DECODED_SIDE);
        limits.max_alloc = Some(MOST_DECODED_BYTES);
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|err| err.to_string())?;
        reader.limits(limits);
        let decoded = reader.decode().map_err(|err| err.to_string())?;
        let small = if decoded.width() > width.max(1) || decoded.height() > height.max(1) {
            decoded.resize(
                width.max(1),
                height.max(1),
                image::imageops::FilterType::Triangle,
            )
        } else {
            decoded
        };
        let mut png = Vec::new();
        small
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|err| err.to_string())?;
        Ok(png)
    })
    .unwrap_or_else(|_| Err("its decoder gave up on it".into()))
}

/// The bytes as base64, as a terminal's graphics protocol takes them.
pub fn to_base64(bytes: &[u8]) -> String {
    base64(bytes)
}

/// The image file at `path`, read in: no larger than [`MOST_FILE_BYTES`], through no link.
///
/// # Errors
///
/// A message when it cannot be read or is too large.
pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let file = crate::providers::open_no_follow(path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let meta = file.metadata().map_err(|err| err.to_string())?;
    if !meta.is_file() {
        return Err(format!("{} is no file", path.display()));
    }
    if meta.len() > MOST_FILE_BYTES {
        return Err(format!(
            "{} is larger than {} — no image is read in that large",
            path.display(),
            size(MOST_FILE_BYTES)
        ));
    }
    let mut bytes = Vec::new();
    file.take(MOST_FILE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(bytes)
}

/// The kept image's type and data as base64, as the APIs take them. Only a file Glut keeps is read ([`kept_media`]) and
/// only an image is sent — a message's reference read from elsewhere (an edited session) opens nothing else.
///
/// # Errors
///
/// A message when the reference is no kept image's or its file is gone.
pub fn base64_of(image: &Image) -> Result<(&'static str, String), String> {
    let media = kept_media(&image.path)
        .ok_or_else(|| format!("image #{} is no image Glut keeps", image.number))?;
    let data = read_file(&image.path).map_err(|_| format!("image #{} is gone", image.number))?;
    if !is_image(&data) {
        return Err(format!("image #{} is no image Glut keeps", image.number));
    }
    Ok((media, base64(&data)))
}

/// The type of a file Glut keeps — named by its hash, in an images folder of a `.glut` — taken from its name, never
/// from what a message says; `None` for any other path.
pub fn kept_media(path: &Path) -> Option<&'static str> {
    let (hash, extension) = path.file_name()?.to_str()?.split_once('.')?;
    let named = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
    let kept = path.is_absolute()
        && path
            .parent()
            .is_some_and(|folder| folder.ends_with(Path::new(crate::providers::DIR).join(DIR)));
    if !named || !kept {
        return None;
    }
    match extension {
        "png" => Some("image/png"),
        "jpg" => Some("image/jpeg"),
        _ => None,
    }
}

/// Whether a file's name says it is an image Glut reads.
pub fn is_image_name(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        })
}

/// An image's width and height, read from its head.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Standard base64, as the file service gives an image, back to its bytes.
pub fn from_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let text = text.trim_end_matches('=').as_bytes();
    // A last group of one character holds no whole byte: no base64 ends so.
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.chunks(4) {
        let mut joined = 0u32;
        for (at, c) in chunk.iter().enumerate() {
            joined |= value(*c)? << (18 - 6 * at);
        }
        let bytes = joined.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let joined = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(char::from(
                    ALPHABET[((joined >> (18 - 6 * at)) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A small image kept in a home of its own, for the tests of the model clients.
#[cfg(test)]
pub(crate) fn kept_for_tests(name: &str) -> (PathBuf, Image) {
    let home = std::env::temp_dir().join(format!("glut-images-{name}-{}", std::process::id()));
    let picture = image::RgbImage::from_pixel(4, 3, image::Rgb([200, 80, 20]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(picture)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    let image = keep(&home, &bytes, 1).unwrap();
    (home, image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(picture)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn base64_is_the_standard_one() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"foobar"] {
            assert_eq!(from_base64(&base64(sample)).unwrap(), sample);
        }
        assert_eq!(from_base64("Zm9v!"), None);
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_large_image_is_kept_scaled_once_by_its_hash_and_read_back() {
        let home = std::env::temp_dir().join(format!("glut-images-{}", std::process::id()));
        let image = keep(&home, &png(1600, 900), 1).unwrap();
        assert_eq!((image.width, image.height), (LONG_SIDE, 882));
        assert_eq!(image.media, "image/png");
        assert!(image.path.starts_with(dir(&home)));
        let again = keep(&home, &png(1600, 900), 2).unwrap();
        assert_eq!(again.path, image.path, "kept once");
        assert!(!base64_of(&image).unwrap().1.is_empty());
        assert!(image.label().starts_with("[image #1 · 1568×882 · "));
        let small = keep(&home, &png(40, 30), 3).unwrap();
        assert_eq!(
            (small.width, small.height),
            (40, 30),
            "a small one stays as it is"
        );
        assert!(keep(&home, b"no image at all", 4).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A PNG whose header claims `width` × `height` and that holds no pixels.
    fn claimed_png(width: u32, height: u32) -> Vec<u8> {
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&[0, 0, 0, 0]);
            out
        };
        let mut header = width.to_be_bytes().to_vec();
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &header));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    #[test]
    fn an_image_claiming_to_be_huge_or_broken_fails_quickly_and_never_panics() {
        let home = std::env::temp_dir().join(format!("glut-images-bomb-{}", std::process::id()));
        let started = std::time::Instant::now();
        assert!(keep(&home, &claimed_png(60_000, 60_000), 1).is_err());
        assert!(keep(&home, &claimed_png(4, 3), 2).is_err(), "no pixels");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_kept_image_comes_back_as_few_pixels() {
        let (home, image) = kept_for_tests("thumbnail");
        let pixels = thumbnail(&image, 2, 2).unwrap();
        assert_eq!((pixels.len(), pixels[0].len()), (2, 2));
        assert_eq!(pixels[0][0], [200, 80, 20]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_reference_to_anything_but_a_kept_image_reads_nothing() {
        let forged = Image {
            number: 1,
            path: PathBuf::from("/etc/passwd"),
            media: "image/png".into(),
            width: 1,
            height: 1,
            bytes: 1,
        };
        assert!(base64_of(&forged).is_err());
    }
}
