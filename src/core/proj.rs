use crate::core::option::TileProj;
use geo::{Coord, Geometry, MapCoordsInPlace, Rect};
use proj::Proj;

/// Serde helper: check if a value equals its default.
pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

// ── WMTS Tile Index → Bounding Rectangle ──

/// Convert a WMTS tile index (x, y, z) to a geographic bounding rectangle.
pub fn get_rect_from_xyz(x: u32, y: u32, z: u32, proj: &TileProj) -> Rect {
    match proj {
        TileProj::EPSG3857 => get_rect_from_xyz_3857(x, y, z),
        TileProj::EPSG4326 => get_rect_from_xyz_4326(x, y, z),
    }
}

/// Compute the EPSG:4326 (WGS84) bounding box for a tile index.
pub fn get_rect_from_xyz_4326(x: u32, y: u32, z: u32) -> Rect {
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
pub fn get_rect_from_xyz_3857(x: u32, y: u32, z: u32) -> Rect {
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

// ── Coordinate Transformation (via proj crate) ──

/// Transform a geometry between EPSG:4326 and EPSG:3857.
pub fn transform(geom: &mut Geometry, proj: &TileProj) {
    match proj {
        TileProj::EPSG3857 => transform_4326_to_3857(geom),
        TileProj::EPSG4326 => transform_3857_to_4326(geom),
    }
}

/// Transform geometry coordinates from EPSG:4326 to EPSG:3857 (Web Mercator).
pub fn transform_4326_to_3857(geom: &mut Geometry) {
    let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .expect("Failed to create EPSG:4326→3857 transformer");
    geom.map_coords_in_place(|Coord { x, y }| -> Coord {
        let (x, y) = transformer.convert((x, y)).unwrap_or((x, y));
        Coord { x, y }
    });
}

/// Transform geometry coordinates from EPSG:3857 to EPSG:4326.
pub fn transform_3857_to_4326(geom: &mut Geometry) {
    let transformer = Proj::new_known_crs("EPSG:3857", "EPSG:4326", None)
        .expect("Failed to create EPSG:3857→4326 transformer");
    geom.map_coords_in_place(|Coord { x, y }| -> Coord {
        let (x, y) = transformer.convert((x, y)).unwrap_or((x, y));
        Coord { x, y }
    });
}

/// Transform a single point from EPSG:4326 to EPSG:3857.
pub fn transform_4326_to_3857_point(x: f64, y: f64) -> (f64, f64) {
    Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .ok()
        .and_then(|t| t.convert((x, y)).ok())
        .unwrap_or((x, y))
}

/// Transform a single point from EPSG:3857 to EPSG:4326.
pub fn transform_3857_to_4326_point(x: f64, y: f64) -> (f64, f64) {
    Proj::new_known_crs("EPSG:3857", "EPSG:4326", None)
        .ok()
        .and_then(|t| t.convert((x, y)).ok())
        .unwrap_or((x, y))
}
