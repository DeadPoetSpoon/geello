//! Explicit bounded output encoding, separate from render execution.
use crate::{Accounted, GeelloError, OperationContext, RgbaImage};
use image::ImageEncoder;
use std::io::Write;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg { quality: u8, background: [u8; 3] },
}
struct Output {
    bytes: Vec<u8>,
    max: usize,
}
impl Write for Output {
    fn write(&mut self, v: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(v.len())
            .is_none_or(|n| n > self.max)
        {
            return Err(std::io::Error::other("encoded output budget exceeded"));
        }
        self.bytes.extend_from_slice(v);
        Ok(v.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn encode(
    image: &RgbaImage,
    format: ImageFormat,
    max_bytes: usize,
    ctx: &OperationContext,
) -> Result<Accounted<Vec<u8>>, GeelloError> {
    ctx.check()?;
    let hold = ctx.reserve(
        max_bytes
            .checked_mul(2)
            .and_then(|v| v.checked_add(image.pixels().len()))
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let mut out = Output {
        bytes: vec![],
        max: max_bytes,
    };
    match format {
        ImageFormat::Png => image::codecs::png::PngEncoder::new(&mut out).write_image(
            image.pixels(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        ),
        ImageFormat::Jpeg {
            quality,
            background,
        } => {
            if quality == 0 || quality > 100 {
                return Err(GeelloError::InvalidConfiguration(
                    "JPEG quality must be 1..100".into(),
                ));
            }
            let mut rgb = Vec::with_capacity(image.pixels().len() / 4 * 3);
            for p in image.pixels().chunks_exact(4) {
                for i in 0..3 {
                    rgb.push(
                        ((p[i] as u32 * p[3] as u32
                            + background[i] as u32 * (255 - p[3] as u32)
                            + 127)
                            / 255) as u8,
                    );
                }
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode(
                &rgb,
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgb8,
            )
        }
    }
    .map_err(|e| GeelloError::Backend(e.to_string()))?;
    ctx.checkpoint((image.width() as u64) * (image.height() as u64))?;
    Ok(Accounted::new(out.bytes, vec![hold]))
}
