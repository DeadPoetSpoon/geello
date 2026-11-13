//! Shared adapters only translate frozen commands, never source geometry or style.
use crate::scene::{PathElement, Stroke};
use crate::{Color, GeelloError, LineCap, LineJoin, RgbaImage};
use vello_common::{
    color::{AlphaColor, Srgb},
    kurbo::{BezPath, Cap, Join},
};

pub(crate) fn path(elements: &[PathElement]) -> BezPath {
    let mut p = BezPath::new();
    for e in elements {
        match *e {
            PathElement::Move(x) => p.move_to((x[0], x[1])),
            PathElement::Line(x) => p.line_to((x[0], x[1])),
            PathElement::Cubic(a, b, c) => p.curve_to((a[0], a[1]), (b[0], b[1]), (c[0], c[1])),
            PathElement::Close => p.close_path(),
        }
    }
    p
}
pub(crate) fn color(c: Color, opacity: f64) -> AlphaColor<Srgb> {
    AlphaColor::new([
        c.0[0] as f32 / 255.0,
        c.0[1] as f32 / 255.0,
        c.0[2] as f32 / 255.0,
        c.0[3] as f32 / 255.0 * opacity as f32,
    ])
}
pub(crate) fn stroke(s: &Stroke) -> vello_common::kurbo::Stroke {
    let cap = match s.cap {
        LineCap::Butt => Cap::Butt,
        LineCap::Round => Cap::Round,
        LineCap::Square => Cap::Square,
    };
    let mut st = vello_common::kurbo::Stroke::new(s.width);
    st.start_cap = cap;
    st.end_cap = cap;
    st.join = match s.join {
        LineJoin::Miter => Join::Miter,
        LineJoin::Round => Join::Round,
        LineJoin::Bevel => Join::Bevel,
    };
    st.miter_limit = s.miter_limit;
    st.dash_pattern = s.dashes.iter().copied().collect();
    st.dash_offset = s.dash_offset;
    st
}
pub(crate) fn premultiply(pixels: &[u8]) -> Vec<u8> {
    let mut p = pixels.to_vec();
    for px in p.chunks_exact_mut(4) {
        for i in 0..3 {
            px[i] = ((px[i] as u32 * px[3] as u32 + 127) / 255) as u8
        }
    }
    p
}
pub(crate) fn straight(
    width: u32,
    height: u32,
    mut pixels: Vec<u8>,
) -> Result<RgbaImage, GeelloError> {
    for p in pixels.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a == 0 {
            p[..3].fill(0)
        } else {
            for c in &mut p[..3] {
                *c = (((*c as u32) * 255 + a / 2) / a).min(255) as u8
            }
        }
    }
    RgbaImage::from_rgba(width, height, pixels)
}
#[cfg(feature = "render-cpu")]
pub(crate) fn image_brush(image: &RgbaImage) -> vello_common::paint::Image {
    use vello_common::peniko;
    use vello_common::{
        paint::ImageSource,
        pixmap::{PixelMetadata, Pixmap},
    };
    let pixmap = Pixmap::from_parts(
        premultiply(image.pixels()),
        image.width() as u16,
        image.height() as u16,
        PixelMetadata::new(peniko::ImageAlphaType::AlphaPremultiplied, true),
    );
    peniko::ImageBrush {
        image: ImageSource::Pixmap(std::sync::Arc::new(pixmap)),
        sampler: peniko::ImageSampler::new().with_quality(peniko::ImageQuality::Low),
    }
}
