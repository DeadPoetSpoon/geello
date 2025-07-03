use crate::render_option::TileProj;
use geo::Rect;
use geo::{Coord, Geometry, MapCoordsInPlace};
const EARTH_RADIUS: f64 = 6378137.0;
const PI: f64 = std::f64::consts::PI;
const EPSG3857_XY_MAX: f64 = EARTH_RADIUS * PI;

pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

pub fn get_rect_from_xyz(x: u32, y: u32, z: u32, proj: &TileProj) -> Rect {
    match proj {
        TileProj::EPSG3857 => get_rect_from_xyz_3857(x, y, z),
        TileProj::EPSG4326 => get_rect_from_xyz_4326(x, y, z),
    }
}
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
pub fn get_rect_from_xyz_3857(x: u32, y: u32, z: u32) -> Rect {
    let n = 2u32.pow(z);
    let deg = EPSG3857_XY_MAX * 2f64 / n as f64;
    let min_lon = x as f64 * deg - EPSG3857_XY_MAX;
    let max_lat = EPSG3857_XY_MAX - y as f64 * deg;
    let max_lon = min_lon + deg;
    let min_lat = max_lat - deg;
    Rect::new((min_lon, min_lat), (max_lon, max_lat))
}

pub fn transform(geom: &mut Geometry, proj: &TileProj) {
    match proj {
        TileProj::EPSG4326 => transform_3857_to_4326(geom),
        TileProj::EPSG3857 => transform_4326_to_3857(geom),
    }
}

pub fn transform_3857_to_4326(geom: &mut Geometry) {
    geom.map_coords_in_place(|Coord { x, y }| -> Coord {
        let (x, y) = transform_3857_to_4326_point(x, y);
        Coord { x, y }
    });
}

pub fn transform_4326_to_3857(geom: &mut Geometry) {
    geom.map_coords_in_place(|Coord { x, y }| -> Coord {
        let (x, y) = transform_4326_to_3857_point(x, y);
        Coord { x, y }
    });
}

pub fn transform_4326_to_3857_point(x: f64, y: f64) -> (f64, f64) {
    let x = x * EPSG3857_XY_MAX / 180f64;
    let y = ((y + 90f64) * PI / 360f64).tan().ln() / (PI / 180f64);
    let y = y * EPSG3857_XY_MAX / 180f64;
    (x, y)
}

pub fn transform_3857_to_4326_point(x: f64, y: f64) -> (f64, f64) {
    let x = x * 180f64 / EPSG3857_XY_MAX;
    let y = y * 180f64 / EPSG3857_XY_MAX;
    let y = ((y * (PI / 180f64)).exp().atan() * 360f64) / PI - 90f64;
    (x, y)
}
