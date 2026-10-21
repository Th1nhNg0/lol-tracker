//! Lossy WebP encoding without redundant full-size RGBA/ARGB buffers.
//!
//! Import directly into the native lossy YUV representation, then release the
//! source pixels before compression allocates its working memory. Settings and
//! conversion match WebPEncodeRGBA; the small FFI boundary owns all C resources.
use anyhow::{Result, ensure};
use libwebp_sys::{WebPConfig, WebPMemoryWriter, WebPPicture};
use std::mem::MaybeUninit;

struct Picture(WebPPicture);
impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: the initialized picture owns its native import allocations.
        unsafe { libwebp_sys::WebPPictureFree(&mut self.0); }
    }
}
struct Writer(WebPMemoryWriter);
impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: the initialized writer exclusively owns its native buffer.
        unsafe { libwebp_sys::WebPMemoryWriterClear(&mut self.0); }
    }
}

pub(crate) fn rgba(pixels: Vec<u8>, width: u32, height: u32, quality: f32) -> Result<Vec<u8>> {
    ensure!((1..=16383).contains(&width) && (1..=16383).contains(&height), "Unsupported WebP dimensions");
    let stride = width.checked_mul(4).ok_or_else(|| anyhow::anyhow!("Image stride overflow"))?;
    let required = (stride as usize).checked_mul(height as usize).ok_or_else(|| anyhow::anyhow!("Image size overflow"))?;
    ensure!(pixels.len() == required, "Invalid RGBA image buffer");
    ensure!(quality.is_finite() && (0.0..=100.0).contains(&quality), "Invalid WebP quality");
    let mut config = WebPConfig::new().map_err(|_| anyhow::anyhow!("Cannot initialize WebP config"))?;
    config.quality = quality;

    let mut raw_picture = MaybeUninit::<WebPPicture>::uninit();
    // SAFETY: the C initializer fully initializes picture storage on success;
    // the matching encoder ABI version is passed explicitly.
    ensure!(unsafe { libwebp_sys::WebPPictureInitInternal(raw_picture.as_mut_ptr(), libwebp_sys::WEBP_ENCODER_ABI_VERSION as i32) } != 0, "Cannot initialize WebP picture");
    let mut picture = Picture(unsafe { raw_picture.assume_init() });
    picture.0.width = width as i32;
    picture.0.height = height as i32;
    // use_argb defaults to zero, so import allocates only lossy YUV samples.
    // SAFETY: dimensions and stride fit C int, and pixels contains every byte
    // addressed by the synchronous import. It does not retain the source pointer.
    ensure!(unsafe { libwebp_sys::WebPPictureImportRGBA(&mut picture.0, pixels.as_ptr(), stride as i32) } != 0, "Cannot import WebP pixels");
    drop(pixels);

    let mut raw_writer = MaybeUninit::<WebPMemoryWriter>::zeroed();
    // Zero the reserved pad too: the C initializer writes the active fields
    // only, so uninitialized storage would not form a valid Rust value.
    unsafe { libwebp_sys::WebPMemoryWriterInit(raw_writer.as_mut_ptr()); }
    let mut writer = Writer(unsafe { raw_writer.assume_init() });
    picture.0.writer = Some(libwebp_sys::WebPMemoryWrite);
    picture.0.custom_ptr = (&mut writer.0 as *mut WebPMemoryWriter).cast();
    // SAFETY: config, picture, and writer remain live at stable addresses during
    // this synchronous call; the callback does not retain its custom pointer.
    ensure!(unsafe { libwebp_sys::WebPEncode(&config, &mut picture.0) } != 0, "Cannot encode WebP image");
    ensure!(!writer.0.mem.is_null() && writer.0.size > 0 && writer.0.size <= isize::MAX as usize, "Invalid WebP output");
    // SAFETY: the successful writer owns size initialized bytes until guard drop.
    Ok(unsafe { std::slice::from_raw_parts(writer.0.mem, writer.0.size) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_output_matches_generic_encoder() {
        for (width, height) in [(1, 1), (13, 17), (128, 64)] {
            let pixels: Vec<u8> = (0..width * height).flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 17) as u8, 255]).collect();
            let expected = webp::Encoder::from_rgba(&pixels, width, height).encode(90.0).to_vec();
            assert_eq!(rgba(pixels, width, height, 90.0).unwrap(), expected);
        }
    }

    #[test]
    fn transparency_and_invalid_input() {
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 50, 60, 70, 255];
        let encoded = rgba(pixels.clone(), 2, 2, 90.0).unwrap();
        let decoded = webp::Decoder::new(&encoded).decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert!(decoded.is_alpha());
        assert_eq!([decoded[3], decoded[7], decoded[11], decoded[15]], [255, 128, 0, 255]);
        assert!(rgba(pixels.clone(), 1, 2, 90.0).is_err());
        assert!(rgba(Vec::new(), 0, 0, 90.0).is_err());
        assert!(rgba(Vec::new(), u32::MAX, 1, 90.0).is_err());
        assert!(rgba(pixels, 2, 2, f32::NAN).is_err());
    }
}
