use std::collections::HashMap;

use vello::Scene;
use vello::kurbo::{Affine, Stroke};
use vello::peniko::{Brush, Fill};

use crate::core::error::GeelloResult;
use crate::core::layer::{LayerId, PropertyValue, RenderedLayer};
use crate::core::lazy::{ResolveContext, opt_resolve};
use crate::core::projection::WorldToPixel;
use crate::core::style::{ResolvedRendererDef, ResolvedStyle};
use crate::render::filter::geometry_passes_filter;
use crate::render::geometry::{
    circle_bezier, fill_paths, geometry_anchors, line_paths, pick_points,
};
use crate::render::parse::{parse_cap, parse_color, parse_join};
use crate::render::text::FontCache;

pub fn build_scene(
    style: &ResolvedStyle,
    layers: &HashMap<LayerId, RenderedLayer>,
    w2p: &WorldToPixel,
    fonts: &FontCache,
    scene: &mut Scene,
) -> GeelloResult<()> {
    let affine = w2p.to_affine();
    for layer_style in &style.layers {
        for (layer_id, layer) in layers {
            if !layer_style
                .filter
                .matches(&layer_id.workspace, &layer_id.name)
            {
                continue;
            }
            for renderer in &layer_style.renderers {
                push_renderer(renderer, layer, &affine, fonts, scene)?;
            }
        }
    }
    Ok(())
}

fn push_renderer(
    r: &ResolvedRendererDef,
    layer: &RenderedLayer,
    affine: &Affine,
    fonts: &FontCache,
    scene: &mut Scene,
) -> GeelloResult<()> {
    let filter = r.geometry_filter.as_ref();

    if let Some(ref cfg) = r.point {
        for g in &layer.geometries {
            if let Some(f) = filter {
                if !geometry_passes_filter(&g.geometry, &g.properties, f) {
                    continue;
                }
            }
            let ctx = ResolveContext {
                properties: Some(&g.properties),
            };
            let radius = opt_resolve(cfg.radius.clone(), &ctx)?.unwrap_or(3.0);
            let c = parse_color(
                opt_resolve(cfg.color.clone(), &ctx)?
                    .as_deref()
                    .unwrap_or("#000000"),
            );
            let bw = opt_resolve(cfg.border_width.clone(), &ctx)?;
            let bc = opt_resolve(cfg.border_color.clone(), &ctx)?;
            for (px, py) in pick_points(&g.geometry) {
                let path = circle_bezier(px, py, radius);
                scene.fill(Fill::NonZero, *affine, &Brush::Solid(c), None, &path);
                if let Some(bw) = bw {
                    let bc = parse_color(bc.as_deref().unwrap_or("#000000"));
                    scene.stroke(&Stroke::new(bw), *affine, &Brush::Solid(bc), None, &path);
                }
            }
        }
    }

    if let Some(ref cfg) = r.line {
        for g in &layer.geometries {
            if let Some(f) = filter {
                if !geometry_passes_filter(&g.geometry, &g.properties, f) {
                    continue;
                }
            }
            let ctx = ResolveContext {
                properties: Some(&g.properties),
            };
            let width = opt_resolve(cfg.width.clone(), &ctx)?.unwrap_or(1.0);
            let c = parse_color(
                opt_resolve(cfg.color.clone(), &ctx)?
                    .as_deref()
                    .unwrap_or("#000000"),
            );
            let join = parse_join(opt_resolve(cfg.join.clone(), &ctx)?.as_deref());
            let cap = parse_cap(opt_resolve(cfg.cap.clone(), &ctx)?.as_deref());
            let miter_limit = opt_resolve(cfg.miter_limit.clone(), &ctx)?.unwrap_or(4.0);
            let mut stroke = Stroke::new(width);
            stroke.join = join;
            stroke.start_cap = cap;
            stroke.end_cap = cap;
            stroke.miter_limit = miter_limit;
            for path in line_paths(&g.geometry) {
                scene.stroke(&stroke, *affine, &Brush::Solid(c), None, &path);
            }
        }
    }

    if let Some(ref cfg) = r.area {
        for g in &layer.geometries {
            if let Some(f) = filter {
                if !geometry_passes_filter(&g.geometry, &g.properties, f) {
                    continue;
                }
            }
            let ctx = ResolveContext {
                properties: Some(&g.properties),
            };
            let c = parse_color(
                opt_resolve(cfg.fill_color.clone(), &ctx)?
                    .as_deref()
                    .unwrap_or("#cccccc"),
            );
            for path in fill_paths(&g.geometry) {
                scene.fill(Fill::NonZero, *affine, &Brush::Solid(c), None, &path);
            }
        }
    }

    if let Some(ref cfg) = r.text {
        for g in &layer.geometries {
            if let Some(f) = filter {
                if !geometry_passes_filter(&g.geometry, &g.properties, f) {
                    continue;
                }
            }
            let ctx = ResolveContext {
                properties: Some(&g.properties),
            };
            let font_key = opt_resolve(cfg.font.clone(), &ctx)?.unwrap_or_else(|| "default".into());
            let label_field =
                opt_resolve(cfg.field.clone(), &ctx)?.unwrap_or_else(|| "name".into());
            let font_size = opt_resolve(cfg.size.clone(), &ctx)?.unwrap_or(12.0);
            let c = parse_color(
                opt_resolve(cfg.color.clone(), &ctx)?
                    .as_deref()
                    .unwrap_or("#000000"),
            );
            let ox = opt_resolve(cfg.offset_x.clone(), &ctx)?.unwrap_or(0.0);
            let oy = opt_resolve(cfg.offset_y.clone(), &ctx)?.unwrap_or(0.0);
            if let Some(PropertyValue::String(label)) = g.properties.get(&label_field) {
                for (ax, ay) in geometry_anchors(&g.geometry) {
                    if let (Some(font), Ok(glyphs)) = (
                        fonts.font_data(&font_key),
                        fonts.layout_glyphs(&font_key, label, ax + ox, ay + oy, font_size),
                    ) {
                        if !glyphs.is_empty() {
                            scene
                                .draw_glyphs(&font)
                                .font_size(font_size as f32)
                                .brush(&Brush::Solid(c))
                                .transform(*affine)
                                .draw(Fill::NonZero, glyphs.into_iter());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
