//! Local taskbar artwork: Claude's icon with the profile avatar in a corner.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use ab_glyph::{point, Font, FontArc};
use image::{DynamicImage, Rgba, RgbaImage};
use sha2::{Digest, Sha256};

use crate::data::Account;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    avatar: Option<(PathBuf, Option<SystemTime>, u64)>,
    name: String,
    color: usize,
}

#[derive(Clone)]
pub struct IconAsset {
    pub path: PathBuf,
    pub digest: String,
}

pub struct IconCache {
    directory: PathBuf,
    base: Option<RgbaImage>,
    entries: HashMap<String, (Source, IconAsset)>,
    // Windows retain the HICON passed in WM_SETICON. Keep every supplied handle
    // alive until the switcher exits, including artwork replaced by a new avatar.
    handles: HashMap<PathBuf, NativeIcon>,
}

struct NativeIcon(windows_sys::Win32::UI::WindowsAndMessaging::HICON);

impl Drop for NativeIcon {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(self.0) };
    }
}

impl IconCache {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            base: None,
            entries: HashMap::new(),
            handles: HashMap::new(),
        }
    }

    pub fn handle(&mut self, asset: &IconAsset) -> io::Result<usize> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            LoadImageW, IMAGE_ICON, LR_LOADFROMFILE,
        };
        if let Some(icon) = self.handles.get(&asset.path) {
            return Ok(icon.0 as usize);
        }
        let path: Vec<u16> = asset
            .path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = unsafe {
            LoadImageW(
                std::ptr::null_mut(),
                path.as_ptr(),
                IMAGE_ICON,
                256,
                256,
                LR_LOADFROMFILE,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        self.handles.insert(asset.path.clone(), NativeIcon(handle));
        Ok(handle as usize)
    }

    pub fn prepare(&mut self, account: &Account, avatar: Option<&Path>) -> io::Result<IconAsset> {
        let source = Source {
            avatar: avatar.and_then(|path| {
                std::fs::metadata(path)
                    .ok()
                    .map(|m| (path.to_owned(), m.modified().ok(), m.len()))
            }),
            name: account.name.clone(),
            color: account.color,
        };
        let key = crate::data::normalize_key(&account.path);
        if let Some((previous, asset)) = self.entries.get(&key) {
            if previous == &source && asset.path.is_file() {
                return Ok(asset.clone());
            }
        }
        let base = self.base.get_or_insert_with(|| {
            RgbaImage::from_raw(256, 256, crate::theme::icon_rgba(256)).expect("square icon")
        });
        let avatar = source
            .avatar
            .as_ref()
            .and_then(|(path, _, _)| crate::identity::read_avatar_image(path).ok());
        let avatar = avatar.unwrap_or_else(|| {
            DynamicImage::ImageRgba8(initial_avatar(&account.name, account.color))
        });
        let image = with_avatar_badge(base, &avatar);
        let bytes = encode_ico(&image)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let path = self.directory.join(format!("{digest}.ico"));
        std::fs::create_dir_all(&self.directory)?;
        if !path.is_file() {
            match crate::storage::write_atomic(&path, &bytes, false) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let asset = IconAsset { path, digest };
        self.entries.insert(key, (source, asset.clone()));
        Ok(asset)
    }
}

fn initial_avatar(name: &str, color: usize) -> RgbaImage {
    let tint = crate::theme::avatar_tint(
        color,
        &crate::theme::Palette::from_mode(crate::theme::Mode::Light),
    );
    let mut image = RgbaImage::from_pixel(128, 128, Rgba([tint.r(), tint.g(), tint.b(), 255]));
    static FONT: OnceLock<Option<FontArc>> = OnceLock::new();
    let font = FONT.get_or_init(|| {
        let root = std::env::var_os("WINDIR").map(PathBuf::from)?.join("Fonts");
        ["msyhbd.ttc", "seguisb.ttf", "segoeui.ttf"]
            .into_iter()
            .find_map(|name| FontArc::try_from_vec(std::fs::read(root.join(name)).ok()?).ok())
    });
    let letter = name
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?');
    if let Some(font) = font {
        let glyph = font
            .glyph_id(letter)
            .with_scale_and_position(80.0, point(0.0, 0.0));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let left = ((128.0 - bounds.width()) * 0.5).round() as i32;
            let top = ((128.0 - bounds.height()) * 0.5).round() as i32;
            outline.draw(|x, y, alpha| {
                let (x, y) = (left + x as i32, top + y as i32);
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let pixel = image.get_pixel_mut(x as u32, y as u32);
                    for channel in &mut pixel.0[..3] {
                        *channel = (*channel as f32 * (1.0 - alpha) + 255.0 * alpha).round() as u8;
                    }
                }
            });
        }
    }
    image
}

fn with_avatar_badge(base: &RgbaImage, avatar: &DynamicImage) -> RgbaImage {
    // Offset the two marks: enlarge the avatar without covering Claude's center.
    let mut out = RgbaImage::new(256, 256);
    let main = image::imageops::resize(base, 196, 196, image::imageops::FilterType::Lanczos3);
    image::imageops::overlay(&mut out, &main, 0, 0);
    let avatar = avatar
        .resize_to_fill(152, 152, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    // 160/256 (62.5%) badge diameter, with a thin contrasting rim. The main
    // icon's center at (98, 98) stays completely outside the avatar circle.
    for y in 96..256 {
        for x in 96..256 {
            let distance =
                ((x as f32 + 0.5 - 176.0).powi(2) + (y as f32 + 0.5 - 176.0).powi(2)).sqrt();
            let coverage = (80.0 - distance).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            let mut badge = Rgba([255, 255, 255, 255]);
            if distance < 76.0 && (100..252).contains(&x) && (100..252).contains(&y) {
                let source = avatar.get_pixel(x - 100, y - 100);
                image::Pixel::blend(&mut badge, source);
            }
            badge.0[3] = (coverage * 255.0).round() as u8;
            image::Pixel::blend(out.get_pixel_mut(x, y), &badge);
        }
    }
    out
}

fn encode_ico(image: &RgbaImage) -> io::Result<Vec<u8>> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let frames: Vec<_> = sizes
        .into_iter()
        .map(|size| {
            let image =
                image::imageops::resize(image, size, size, image::imageops::FilterType::Lanczos3);
            let mut png = Cursor::new(Vec::new());
            DynamicImage::ImageRgba8(image)
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(io::Error::other)?;
            Ok((size, png.into_inner()))
        })
        .collect::<io::Result<_>>()?;
    // ICO supports PNG-compressed frames; no additional image decoder is needed.
    let mut bytes = vec![0, 0, 1, 0];
    bytes.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut offset = 6 + frames.len() * 16;
    for (size, png) in &frames {
        bytes.extend_from_slice(&[*size as u8, *size as u8, 0, 0]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&(png.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in frames {
        bytes.extend_from_slice(&png);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_badge_keeps_claude_mark_and_ico_frames_decode_at_taskbar_sizes() {
        let base = RgbaImage::from_pixel(256, 256, Rgba([200, 80, 60, 255]));
        let avatar =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(100, 200, Rgba([20, 100, 220, 255])));
        let composed = with_avatar_badge(&base, &avatar);
        assert_eq!(composed.get_pixel(60, 60), base.get_pixel(60, 60));
        assert_eq!(composed.get_pixel(98, 98), base.get_pixel(98, 98));
        assert_eq!(composed.get_pixel(192, 192).0, [20, 100, 220, 255]);
        assert_eq!(composed.get_pixel(106, 176).0, [20, 100, 220, 255]);
        let bytes = encode_ico(&composed).unwrap();
        assert_eq!(&bytes[..6], &[0, 0, 1, 0, 7, 0]);
        for index in 0..7 {
            let entry = &bytes[6 + index * 16..6 + (index + 1) * 16];
            let size = if entry[0] == 0 { 256 } else { entry[0] as u32 };
            let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            let decoded = image::load_from_memory_with_format(
                &bytes[offset..offset + length],
                image::ImageFormat::Png,
            )
            .unwrap();
            assert_eq!((decoded.width(), decoded.height()), (size, size));
        }
    }

    #[test]
    fn changed_avatar_publishes_new_content_addressed_icon() {
        let dir = crate::test_support::TestDir::new();
        let avatar = dir.path().join("avatar.png");
        RgbaImage::from_pixel(20, 20, Rgba([255u8, 0, 0, 255]))
            .save(&avatar)
            .unwrap();
        let account = Account {
            id: "test".into(),
            name: "副号".into(),
            path: dir.path().join("profile"),
            is_default: false,
            color: 0,
            running_pid: None,
            last_used: None,
            session_count: 0,
            data_path: None,
        };
        let mut cache = IconCache::new(dir.path().join("icons"));
        cache.base = Some(RgbaImage::from_pixel(256, 256, Rgba([200, 80, 60, 255])));
        let first = cache.prepare(&account, Some(&avatar)).unwrap();
        assert_eq!(
            cache.prepare(&account, Some(&avatar)).unwrap().path,
            first.path
        );
        RgbaImage::from_pixel(24, 24, Rgba([0u8, 0, 255, 255]))
            .save(&avatar)
            .unwrap();
        let second = cache.prepare(&account, Some(&avatar)).unwrap();
        assert_ne!(first.digest, second.digest);
        assert!(first.path.is_file());
        assert!(second.path.is_file());
    }
}
