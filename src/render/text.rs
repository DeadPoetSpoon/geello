use std::collections::HashMap;

use skrifa::{FontRef, MetadataProvider, instance::Location, instance::Size};

use crate::core::error::{GeelloError, GeelloResult};

// ---------------------------------------------------------------------------
// FontCache
// ---------------------------------------------------------------------------

/// Cached font bytes keyed by logical name.
pub struct FontCache {
    fonts: HashMap<String, Vec<u8>>,
}

impl Default for FontCache {
    fn default() -> Self {
        Self::new()
    }
}

impl FontCache {
    pub fn new() -> Self {
        Self {
            fonts: HashMap::new(),
        }
    }

    /// Load a font file and cache its bytes under `key`.
    /// Validates the font data with skrifa before caching.
    pub fn load(&mut self, key: &str, path: &str) -> GeelloResult<()> {
        let data = std::fs::read(path)?;
        FontRef::new(&data)
            .map_err(|e| GeelloError::Render(format!("Failed to parse font '{key}': {e}")))?;
        self.fonts.insert(key.to_owned(), data);
        Ok(())
    }

    /// Return the raw font bytes for `key`.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.fonts.get(key).map(|v| v.as_slice())
    }

    /// Compute glyph positions for `text` starting at `(x, y)`.
    ///
    /// Returns glyph IDs and positions ready for `vello::Scene::draw_glyphs`.
    pub fn layout_glyphs(
        &self,
        font_key: &str,
        text: &str,
        x: f64,
        y: f64,
        font_size: f64,
    ) -> GeelloResult<(Vec<u8>, Vec<vello::Glyph>)> {
        let data = self
            .fonts
            .get(font_key)
            .ok_or_else(|| GeelloError::Render(format!("Font '{font_key}' not loaded")))?;

        let font_ref = FontRef::new(data)
            .map_err(|e| GeelloError::Render(format!("Font parse error: {e}")))?;

        let upem = {
            let loc = Location::default();
            font_ref.metrics(Size::unscaled(), &loc).units_per_em as f64
        };
        let scale = font_size / upem;

        let charmap = font_ref.charmap();
        let loc = Location::default();
        let glyph_metrics = font_ref.glyph_metrics(Size::unscaled(), &loc);

        let mut pen_x = x;
        let glyphs: Vec<vello::Glyph> = text
            .chars()
            .filter_map(|c| {
                let gid = charmap.map(c)?;
                let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64 * scale;
                let glyph = vello::Glyph {
                    id: gid.to_u32(),
                    x: pen_x as f32,
                    y: y as f32,
                };
                pen_x += advance;
                Some(glyph)
            })
            .collect();

        Ok((data.clone(), glyphs))
    }
}
