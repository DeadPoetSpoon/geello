use geo_types::{
    CoordNum, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use vello::kurbo::{BezPath, PathEl};

// ---------------------------------------------------------------------------
// Point → Circle BezPath
// ---------------------------------------------------------------------------

/// kappa = (4/3) * (√2 − 1) — magic constant for approximating a circle
/// with four cubic Bézier segments with 0.027253 % error.
const CIRCLE_KAPPA: f64 = 0.552_284_749_8;

/// Approximate a circle of `radius` centred at `(cx, cy)` as a closed [`BezPath`].
pub fn circle_bezier(cx: f64, cy: f64, radius: f64) -> BezPath {
    let r = radius.max(0.0);
    let k = r * CIRCLE_KAPPA;
    BezPath::from_vec(vec![
        PathEl::MoveTo((cx + r, cy).into()),
        PathEl::CurveTo(
            (cx + r, cy + k).into(),
            (cx + k, cy + r).into(),
            (cx, cy + r).into(),
        ),
        PathEl::CurveTo(
            (cx - k, cy + r).into(),
            (cx - r, cy + k).into(),
            (cx - r, cy).into(),
        ),
        PathEl::CurveTo(
            (cx - r, cy - k).into(),
            (cx - k, cy - r).into(),
            (cx, cy - r).into(),
        ),
        PathEl::CurveTo(
            (cx + k, cy - r).into(),
            (cx + r, cy - k).into(),
            (cx + r, cy).into(),
        ),
        PathEl::ClosePath,
    ])
}

// ---------------------------------------------------------------------------
// LineString → BezPath
// ---------------------------------------------------------------------------

/// Convert a [`LineString`] to an open [`BezPath`] (no `ClosePath`).
pub fn linestring_to_bezpath<T: CoordNum + Into<f64>>(ls: &LineString<T>) -> BezPath {
    let mut path = BezPath::new();
    let mut coords = ls.points();
    if let Some(first) = coords.next() {
        path.move_to(to_kurbo_point(first));
        for p in coords {
            path.line_to(to_kurbo_point(p));
        }
    }
    path
}

/// Convert a closed ring (exterior or interior of a polygon) to a [`BezPath`]
/// with a final `ClosePath`.
fn ring_to_bezpath<T: CoordNum + Into<f64>>(ls: &LineString<T>) -> BezPath {
    let mut path = linestring_to_bezpath(ls);
    path.close_path();
    path
}

// ---------------------------------------------------------------------------
// Polygon → BezPath
// ---------------------------------------------------------------------------

/// Convert a [`Polygon`] to a [`BezPath`].
///
/// The exterior ring is drawn first, then each interior ring (hole) is
/// added with opposite winding so that vello's fill rule correctly
/// punches out the holes.
pub fn polygon_to_bezpath<T: CoordNum + Into<f64>>(poly: &Polygon<T>) -> BezPath {
    let mut path = ring_to_bezpath(poly.exterior());
    for interior in poly.interiors() {
        let hole = ring_to_bezpath(interior);
        // Reverse the hole path so it winds opposite to the exterior.
        let reversed: Vec<PathEl> = hole
            .elements()
            .iter()
            .rev()
            .map(|el| match el {
                PathEl::MoveTo(p) => PathEl::LineTo(*p),
                PathEl::LineTo(p) => PathEl::LineTo(*p),
                PathEl::CurveTo(p1, p2, p) => PathEl::CurveTo(*p2, *p1, *p),
                PathEl::ClosePath => PathEl::MoveTo(
                    hole.elements()
                        .first()
                        .map(|e| e.end_point())
                        .flatten()
                        .unwrap_or_default(),
                ),
                PathEl::QuadTo(p1, p) => PathEl::QuadTo(*p1, *p),
            })
            .collect();
        for el in reversed {
            path.push(el);
        }
        path.close_path();
    }
    path
}

// ---------------------------------------------------------------------------
// Multi-geometry converters
// ---------------------------------------------------------------------------

pub fn multipoint_to_bezpath<T: CoordNum + Into<f64>>(mp: &MultiPoint<T>, radius: f64) -> BezPath {
    let mut path = BezPath::new();
    for p in &mp.0 {
        let circle = circle_bezier(p.x().into(), p.y().into(), radius);
        extend_path(&mut path, &circle);
    }
    path
}

pub fn multilinestring_to_bezpath<T: CoordNum + Into<f64>>(mls: &MultiLineString<T>) -> BezPath {
    let mut path = BezPath::new();
    for ls in &mls.0 {
        extend_path(&mut path, &linestring_to_bezpath(ls));
    }
    path
}

pub fn multipolygon_to_bezpath<T: CoordNum + Into<f64>>(mp: &MultiPolygon<T>) -> BezPath {
    let mut path = BezPath::new();
    for poly in &mp.0 {
        extend_path(&mut path, &polygon_to_bezpath(poly));
    }
    path
}

// ---------------------------------------------------------------------------
// Generic geometry → BezPath dispatcher
// ---------------------------------------------------------------------------

/// Convert any [`Geometry`] variant into a [`BezPath`].
///
/// `point_radius` is used for `Point` / `MultiPoint` geometries.
/// Other geometry types ignore it.
pub fn geometry_to_bezpath<T: CoordNum + Into<f64>>(
    geom: &Geometry<T>,
    point_radius: f64,
) -> BezPath {
    match geom {
        Geometry::Point(p) => circle_bezier(p.x().into(), p.y().into(), point_radius),
        Geometry::Line(line) => {
            let mut path = BezPath::new();
            path.move_to((line.start.x.into(), line.start.y.into()));
            path.line_to((line.end.x.into(), line.end.y.into()));
            path
        }
        Geometry::LineString(ls) => linestring_to_bezpath(ls),
        Geometry::Polygon(poly) => polygon_to_bezpath(poly),
        Geometry::MultiPoint(mp) => multipoint_to_bezpath(mp, point_radius),
        Geometry::MultiLineString(mls) => multilinestring_to_bezpath(mls),
        Geometry::MultiPolygon(mp) => multipolygon_to_bezpath(mp),
        Geometry::Rect(r) => {
            let min = r.min();
            let max = r.max();
            let mut path = BezPath::new();
            path.move_to((min.x.into(), min.y.into()));
            path.line_to((max.x.into(), min.y.into()));
            path.line_to((max.x.into(), max.y.into()));
            path.line_to((min.x.into(), max.y.into()));
            path.close_path();
            path
        }
        Geometry::Triangle(t) => {
            let v1 = t.v1();
            let v2 = t.v2();
            let v3 = t.v3();
            let mut path = BezPath::new();
            path.move_to((v1.x.into(), v1.y.into()));
            path.line_to((v2.x.into(), v2.y.into()));
            path.line_to((v3.x.into(), v3.y.into()));
            path.close_path();
            path
        }
        Geometry::GeometryCollection(gc) => {
            let mut path = BezPath::new();
            for inner in &gc.0 {
                extend_path(&mut path, &geometry_to_bezpath(inner, point_radius));
            }
            path
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_kurbo_point<T: CoordNum + Into<f64>>(p: Point<T>) -> vello::kurbo::Point {
    vello::kurbo::Point::new(p.x().into(), p.y().into())
}

fn extend_path(target: &mut BezPath, source: &BezPath) {
    for el in source.elements() {
        target.push(*el);
    }
}
