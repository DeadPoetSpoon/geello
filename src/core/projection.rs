use geo::Transform;
use geo::proj::Proj;
use geo_types::Geometry;

use crate::core::error::{GeelloError, GeelloResult};
use crate::core::layer::{Crs, RenderedLayer};

/// Supported render-target coordinate reference systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCrs {
    /// WGS84 (EPSG:4326) — latitude / longitude.
    Epsg4326,
    /// Web Mercator (EPSG:3857) — metre-based flat projection.
    Epsg3857,
}

impl TargetCrs {
    fn epsg_code(self) -> u16 {
        match self {
            TargetCrs::Epsg4326 => 4326,
            TargetCrs::Epsg3857 => 3857,
        }
    }

    fn proj_string(self) -> String {
        format!("EPSG:{}", self.epsg_code())
    }
}

/// Build a [`Proj`] instance that transforms from `source` to `target`.
fn build_proj(source: Crs, target: TargetCrs) -> GeelloResult<Proj> {
    let from = format!("EPSG:{}", source.epsg);
    let to = target.proj_string();
    Proj::new_known_crs(&from, &to, None).map_err(|e| {
        GeelloError::Projection(format!(
            "Failed to create projection EPSG:{} → EPSG:{}: {e}",
            source.epsg,
            target.epsg_code(),
        ))
    })
}

// ---------------------------------------------------------------------------
// Per-geometry-type transformation
// ---------------------------------------------------------------------------

macro_rules! transform_via_proj {
    ($geom:expr, $proj:expr) => {
        *$geom = $geom
            .transformed($proj)
            .map_err(|e| GeelloError::Projection(e.to_string()))?
    };
}

/// Transform **every** geometry in a [`RenderedLayer`] from its source CRS
/// to the given `target` CRS, and update the layer's [`Crs`] accordingly.
///
/// Geometries whose source CRS already matches the target are left unchanged.
///
/// This is the primary entry point — call it once after loading a layer
/// and before building the vello scene.
pub fn transform_layer(layer: &mut RenderedLayer, target: TargetCrs) -> GeelloResult<()> {
    if layer.crs.epsg == target.epsg_code() {
        return Ok(());
    }
    let source = layer.crs;
    let proj = build_proj(source, target)?;
    for geom in layer.geometries.iter_mut() {
        transform_geometry(&mut geom.geometry, &proj)?;
    }
    layer.crs = Crs {
        epsg: target.epsg_code(),
    };
    Ok(())
}

fn transform_geometry(geom: &mut Geometry<f64>, proj: &Proj) -> GeelloResult<()> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => transform_via_proj!(p, proj),
        Line(p) => transform_via_proj!(p, proj),
        LineString(p) => transform_via_proj!(p, proj),
        Polygon(p) => transform_via_proj!(p, proj),
        MultiPoint(p) => transform_via_proj!(p, proj),
        MultiLineString(p) => transform_via_proj!(p, proj),
        MultiPolygon(p) => transform_via_proj!(p, proj),
        Rect(p) => transform_via_proj!(p, proj),
        Triangle(p) => transform_via_proj!(p, proj),
        GeometryCollection(gc) => {
            for inner in gc.iter_mut() {
                transform_geometry(inner, proj)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// WMTS tile index → geographic rectangle
// ---------------------------------------------------------------------------

use geo_types::Rect;

/// Compute the EPSG:4326 (WGS84) bounding box for a tile index.
pub fn tile_to_rect_4326(x: u32, y: u32, z: u32) -> Rect {
    if z == 0 {
        if x == 0 {
            Rect::new((-180f64, -90f64), (0f64, 90f64))
        } else if x == 1 {
            Rect::new((0f64, -90f64), (180f64, 90f64))
        } else {
            Rect::new((-182f64, -92f64), (-181f64, -91f64))
        }
    } else {
        let x_n = 2u32.pow(z + 1);
        let y_n = 2u32.pow(z);
        let lon_deg = 360.0 / x_n as f64;
        let lat_deg = 180.0 / y_n as f64;
        let min_lon = x as f64 * lon_deg - 180.0;
        let max_lat = 90.0 - y as f64 * lat_deg;
        let max_lon = min_lon + lon_deg;
        let min_lat = max_lat - lat_deg;
        Rect::new((min_lon, min_lat), (max_lon, max_lat))
    }
}

/// Compute the EPSG:3857 (Web Mercator) bounding box for a tile index.
pub fn tile_to_rect_3857(x: u32, y: u32, z: u32) -> Rect {
    const EARTH_RADIUS: f64 = 6_378_137.0;
    const EPSG3857_XY_MAX: f64 = EARTH_RADIUS * std::f64::consts::PI;

    let n = 2u32.pow(z);
    let deg = EPSG3857_XY_MAX * 2f64 / n as f64;
    let min_x = x as f64 * deg - EPSG3857_XY_MAX;
    let max_y = EPSG3857_XY_MAX - y as f64 * deg;
    let max_x = min_x + deg;
    let min_y = max_y - deg;
    Rect::new((min_x, min_y), (max_x, max_y))
}

// ---------------------------------------------------------------------------
// World → pixel affine transform
// ---------------------------------------------------------------------------

/// Affine transform mapping world coordinates to pixel coordinates.
///
/// The y-axis is flipped: world y increases upward, pixel y increases downward.
#[derive(Debug, Clone, Copy)]
pub struct WorldToPixel {
    scale_x: f64,
    scale_y: f64,
    offset_x: f64,
    offset_y: f64,
}

impl WorldToPixel {
    /// Build a transform for a tile or viewport.
    pub fn new(rect: &Rect, width: u32, height: u32) -> Self {
        let w = width as f64;
        let h = height as f64;
        let min = rect.min();
        let max = rect.max();
        let scale_x = w / (max.x - min.x);
        let scale_y = -h / (max.y - min.y);
        Self {
            scale_x,
            scale_y,
            offset_x: -min.x * scale_x,
            offset_y: max.y * -scale_y,
        }
    }

    /// Transform a single world coordinate to pixel space.
    pub fn transform(&self, world_x: f64, world_y: f64) -> (f64, f64) {
        (
            world_x * self.scale_x + self.offset_x,
            world_y * self.scale_y + self.offset_y,
        )
    }
}
