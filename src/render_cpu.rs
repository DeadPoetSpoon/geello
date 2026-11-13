//! Synchronous Vello CPU execution, without IO, style evaluation or Tokio.
use crate::{GeelloError, OperationContext, PreparedScene, RgbaImage};
use crate::{backend, scene::Command};
use vello_common::{
    kurbo::{Affine, Rect},
    peniko::Fill,
};
use vello_cpu::{Pixmap, RenderContext, RenderSettings, Resources};

/// A reusable CPU adapter. Thread count is explicit and never creates a global pool.
pub struct CpuRenderer {
    resources: Resources,
    threads: u16,
}
impl CpuRenderer {
    pub fn new(threads: u16) -> Self {
        Self {
            resources: Resources::new(),
            threads,
        }
    }
    pub fn render(
        &mut self,
        scene: &PreparedScene,
        context: &OperationContext,
    ) -> Result<RgbaImage, GeelloError> {
        context.check()?;
        let w = scene.summary().width;
        let h = scene.summary().height;
        let bytes = crate::core::pixel_bytes(w, h)?;
        let _memory = context.reserve(bytes.checked_mul(4).ok_or(GeelloError::SizeOverflow)?)?;
        context.checkpoint((w as u64) * (h as u64))?;
        let mut render = RenderContext::new_with(
            w as u16,
            h as u16,
            RenderSettings {
                num_threads: self.threads,
                ..Default::default()
            },
        );
        render.set_fill_rule(Fill::EvenOdd);
        for command in &scene.inner.commands {
            context.checkpoint(1)?;
            match command {
                Command::Path {
                    path,
                    color,
                    opacity,
                    stroke,
                } => {
                    render.set_paint(backend::color(*color, *opacity));
                    render.set_paint_transform(Affine::IDENTITY);
                    let p = backend::path(path);
                    if let Some(st) = stroke {
                        render.set_stroke(backend::stroke(st));
                        render.stroke_path(&p)
                    } else {
                        render.fill_path(&p)
                    }
                }
                Command::Image {
                    image,
                    rect,
                    opacity,
                } => {
                    let brush = backend::image_brush(image).with_alpha(*opacity as f32);
                    render.set_paint(brush);
                    render.set_paint_transform(Affine::new([
                        (rect[2] - rect[0]) / image.width() as f64,
                        0.0,
                        0.0,
                        (rect[3] - rect[1]) / image.height() as f64,
                        rect[0],
                        rect[1],
                    ]));
                    render.fill_rect(&Rect::new(rect[0], rect[1], rect[2], rect[3]));
                }
            }
        }
        context.check()?;
        render.flush();
        let mut target = Pixmap::new(w as u16, h as u16);
        render.render(&mut target, &mut self.resources);
        context.check()?;
        backend::straight(w, h, target.data_as_u8_slice().to_vec())
    }
}
