//! TextRenderer — renders text labels on the map.
//!
//! Uses skrifa for font loading and glyph outline extraction,
//! vello for GPU-accelerated path rendering.
//!
//! Supports font fallback chain: fonts are tried in config order,
//! the first font that contains the required glyph is used.

use std::collections::HashMap;
use std::sync::Arc;

use geo::Point;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use vello::Scene;
use vello::kurbo::{Affine, BezPath};
use vello::peniko::Brush;

use crate::core::value::{LazyValue, MagicConverter, MagicFetcher, PropValue};

/// Cached font data
#[derive(Debug, Clone)]
struct FontCache {
    data: Arc<Vec<u8>>,
}

/// Text anchor point relative to the geometry position
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum TextAnchor {
    #[default]
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
}

/// Text anchor point relative to the geometry position
/// TextRenderer configuration
///
/// Renders a text label at or near a geometry point.
/// Supports font fallback: `font_paths` is tried in order for each character.
/// Text content can be a fixed string or read from GeoJSON properties.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextRenderer {
    /// Font file paths (fallback chain, from assets directory)
    #[serde(default)]
    pub font_paths: Vec<String>,

    /// Font size in world units
    #[serde(default = "default_font_size")]
    pub font_size: f64,

    /// Text content: fixed value or from GeoJSON property
    #[serde(default)]
    pub text: LazyValue<PropValue>,

    /// Text color / brush
    #[serde(default)]
    pub brush: LazyValue<Brush>,

    /// Horizontal offset from anchor point (world units)
    #[serde(default)]
    pub offset_x: f64,

    /// Vertical offset from anchor point (world units)
    #[serde(default)]
    pub offset_y: f64,

    /// Text anchor point (relative to geometry position)
    #[serde(default)]
    pub anchor: TextAnchor,

    /// Cached font data, loaded at fetch time
    #[serde(skip)]
    font_cache: Arc<std::sync::Mutex<Vec<FontCache>>>,
}

fn default_font_size() -> f64 {
    1.0
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self {
            font_paths: Vec::new(),
            font_size: 1.0,
            text: LazyValue::default(),
            brush: Brush::Solid(vello::peniko::color::palette::css::BLACK).into(),
            offset_x: 0.0,
            offset_y: 0.0,
            anchor: TextAnchor::default(),
            font_cache: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl MagicFetcher for TextRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        self.text.fetch()?;
        self.brush.fetch()?;

        // Load font files into cache
        let mut cache = self.font_cache.lock().map_err(|e| e.to_string())?;
        cache.clear();
        for path in &self.font_paths {
            let data = std::fs::read(path)
                .map_err(|e| format!("Failed to load font '{}': {}", path, e))?;
            cache.push(FontCache {
                data: Arc::new(data),
            });
        }
        Ok(())
    }
}

impl MagicConverter for TextRenderer {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        self.text.convert(props)?;
        self.brush.convert(props)?;
        Ok(())
    }
}

impl TextRenderer {
    /// Get the resolved text string
    pub fn text_string(&self) -> Result<String, String> {
        self.text.inner_try_into::<String>()
    }

    /// Draw text anchored at the given point
    pub fn draw(&self, scene: &mut Scene, transform: Affine, point: &Point) -> Result<(), String> {
        let text = self.text_string()?;
        if text.is_empty() {
            return Ok(());
        }

        let brush = self.brush.as_ref();
        let font_cache = self.font_cache.lock().map_err(|e| e.to_string())?;

        if font_cache.is_empty() {
            return Err("No fonts loaded for TextRenderer".to_string());
        }

        // Calculate anchor offset
        let (anchor_x, anchor_y) = self.anchor_offset(&text);
        let base_x = point.x() + self.offset_x + anchor_x;
        let base_y = point.y() + self.offset_y + anchor_y;

        // Render each character
        let mut cursor_x = base_x;
        for ch in text.chars() {
            if let Some(path) = self.glyph_path(ch, &font_cache) {
                let glyph_transform = transform
                    * Affine::translate((cursor_x, base_y))
                    * Affine::scale(self.font_size);

                scene.fill(
                    vello::peniko::Fill::NonZero,
                    glyph_transform,
                    brush,
                    None,
                    &path,
                );
            }
            // Advance cursor (approximate advance width)
            cursor_x += self.font_size * 0.6;
        }

        Ok(())
    }

    /// Get the vello BezPath for a character, using font fallback chain.
    fn glyph_path(&self, ch: char, fonts: &[FontCache]) -> Option<BezPath> {
        for font in fonts {
            let font_ref = FontRef::new(&font.data).ok()?;

            // Map character to glyph ID
            let glyph_id = font_ref.charmap().map(ch)?;
            if glyph_id == GlyphId::NOTDEF {
                continue;
            }

            // TODO: Full glyph outline extraction via skrifa::outline API.
            // For now, use a rectangle placeholder to verify text positioning works.
            return Some(self.placeholder_glyph());
        }
        None
    }

    /// Placeholder glyph shape (rectangle) until full glyph extraction is implemented.
    fn placeholder_glyph(&self) -> BezPath {
        let mut path = BezPath::new();
        let w = 0.5;
        let h = 0.7;
        path.move_to((0.0, 0.0));
        path.line_to((w, 0.0));
        path.line_to((w, h));
        path.line_to((0.0, h));
        path.close_path();
        path
    }

    /// Calculate offset based on text anchor and rendered width
    fn anchor_offset(&self, text: &str) -> (f64, f64) {
        let char_count = text.chars().count() as f64;
        let approx_width = char_count * self.font_size * 0.6;
        let approx_height = self.font_size * 0.7;

        match self.anchor {
            TextAnchor::Center => (-approx_width / 2.0, approx_height / 2.0),
            TextAnchor::TopLeft => (0.0, approx_height),
            TextAnchor::TopRight => (-approx_width, approx_height),
            TextAnchor::BottomLeft => (0.0, 0.0),
            TextAnchor::BottomRight => (-approx_width, 0.0),
            TextAnchor::Left => (0.0, approx_height / 2.0),
            TextAnchor::Right => (-approx_width, approx_height / 2.0),
        }
    }
}
