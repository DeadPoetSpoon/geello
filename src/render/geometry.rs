use geo::algorithm::Centroid;
use geo_types::{
    CoordNum, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use vello::kurbo::{BezPath, PathEl};

// ---------------------------------------------------------------------------
// Point → Circle BezPath
// ---------------------------------------------------------------------------

const CIRCLE_KAPPA: f64 = 0.552_284_749_8;

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

fn ring_to_bezpath<T: CoordNum + Into<f64>>(ls: &LineString<T>) -> BezPath {
    let mut path = linestring_to_bezpath(ls);
    path.close_path();
    path
}

// ---------------------------------------------------------------------------
// Polygon → BezPath
// ---------------------------------------------------------------------------

pub fn polygon_to_bezpath<T: CoordNum + Into<f64>>(poly: &Polygon<T>) -> BezPath {
    let mut path = ring_to_bezpath(poly.exterior());
    for interior in poly.interiors() {
        let hole = ring_to_bezpath(interior);
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
        extend_path(
            &mut path,
            &circle_bezier(p.x().into(), p.y().into(), radius),
        );
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
// Pick points / line paths / fill paths / anchors
// ---------------------------------------------------------------------------

pub fn pick_points(geom: &Geometry<f64>) -> Vec<(f64, f64)> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => vec![(p.x(), p.y())],
        Line(line) => {
            let sx = line.start.x;
            let sy = line.start.y;
            let ex = line.end.x;
            let ey = line.end.y;
            vec![(sx, sy), ((sx + ex) / 2.0, (sy + ey) / 2.0), (ex, ey)]
        }
        LineString(ls) => pick_ls_points(ls),
        Polygon(poly) => pick_ls_points(poly.exterior()),
        MultiPoint(mp) => mp.0.iter().map(|p| (p.x(), p.y())).collect(),
        MultiLineString(mls) => mls.0.iter().flat_map(|ls| pick_ls_points(ls)).collect(),
        MultiPolygon(mp) => {
            mp.0.iter()
                .flat_map(|p| pick_ls_points(p.exterior()))
                .collect()
        }
        Rect(r) => {
            let c = r.min();
            let c2 = r.max();
            vec![
                (c.x, c.y),
                ((c.x + c2.x) / 2.0, (c.y + c2.y) / 2.0),
                (c2.x, c2.y),
            ]
        }
        Triangle(t) => {
            let v1 = t.v1();
            let v2 = t.v2();
            let v3 = t.v3();
            vec![(v1.x, v1.y), (v2.x, v2.y), (v3.x, v3.y)]
        }
        GeometryCollection(gc) => gc.0.iter().flat_map(|g| pick_points(g)).collect(),
    }
}

fn pick_ls_points(ls: &LineString<f64>) -> Vec<(f64, f64)> {
    let pts: Vec<_> = ls.points().collect();
    match pts.len() {
        0 => vec![],
        1 => vec![(pts[0].x(), pts[0].y())],
        n => {
            let mid = n / 2;
            vec![
                (pts[0].x(), pts[0].y()),
                (pts[mid].x(), pts[mid].y()),
                (pts[n - 1].x(), pts[n - 1].y()),
            ]
        }
    }
}

pub fn line_paths(geom: &Geometry<f64>) -> Vec<BezPath> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => vec![circle_bezier(p.x(), p.y(), 1.0)],
        Line(_) | LineString(_) => vec![geometry_to_bezpath(geom, 0.0)],
        Polygon(poly) => {
            let mut paths = vec![linestring_to_bezpath(poly.exterior())];
            for i in poly.interiors() {
                paths.push(linestring_to_bezpath(i));
            }
            paths
        }
        MultiPoint(mp) => {
            mp.0.iter()
                .map(|p| circle_bezier(p.x(), p.y(), 1.0))
                .collect()
        }
        MultiLineString(mls) => mls.0.iter().map(|ls| linestring_to_bezpath(ls)).collect(),
        MultiPolygon(mp) => {
            mp.0.iter()
                .flat_map(|p| line_paths(&Geometry::Polygon(p.clone())))
                .collect()
        }
        Rect(_) | Triangle(_) => vec![geometry_to_bezpath(geom, 0.0)],
        GeometryCollection(gc) => gc.0.iter().flat_map(|g| line_paths(g)).collect(),
    }
}

pub fn fill_paths(geom: &Geometry<f64>) -> Vec<BezPath> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => vec![circle_bezier(p.x(), p.y(), 3.0)],
        Line(line) => {
            let mut path = BezPath::new();
            path.move_to((line.start.x, line.start.y));
            path.line_to((line.end.x, line.end.y));
            path.close_path();
            vec![path]
        }
        LineString(ls) => {
            if ls.points().count() >= 3 {
                vec![closed_bezpath(ls)]
            } else {
                vec![]
            }
        }
        Polygon(_) | Rect(_) | Triangle(_) => vec![geometry_to_bezpath(geom, 0.0)],
        MultiPoint(mp) => {
            mp.0.iter()
                .map(|p| circle_bezier(p.x(), p.y(), 3.0))
                .collect()
        }
        MultiLineString(mls) => mls
            .0
            .iter()
            .filter_map(|ls| {
                if ls.points().count() >= 3 {
                    Some(closed_bezpath(ls))
                } else {
                    None
                }
            })
            .collect(),
        MultiPolygon(mp) => {
            mp.0.iter()
                .map(|p| geometry_to_bezpath(&Geometry::Polygon(p.clone()), 0.0))
                .collect()
        }
        GeometryCollection(gc) => gc.0.iter().flat_map(|g| fill_paths(g)).collect(),
    }
}

fn closed_bezpath(ls: &LineString<f64>) -> BezPath {
    let mut p = linestring_to_bezpath(ls);
    p.close_path();
    p
}

pub fn geometry_anchors(geom: &Geometry<f64>) -> Vec<(f64, f64)> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => vec![(p.x(), p.y())],
        Line(line) => vec![(
            (line.start.x + line.end.x) / 2.0,
            (line.start.y + line.end.y) / 2.0,
        )],
        LineString(ls) => ls
            .centroid()
            .map(|c| vec![(c.x(), c.y())])
            .unwrap_or_default(),
        Polygon(poly) => poly
            .exterior()
            .centroid()
            .map(|c| vec![(c.x(), c.y())])
            .unwrap_or_default(),
        MultiPoint(mp) => mp.0.iter().map(|p| (p.x(), p.y())).collect(),
        MultiLineString(mls) => mls
            .0
            .iter()
            .filter_map(|ls| ls.centroid().map(|c| (c.x(), c.y())))
            .collect(),
        MultiPolygon(mp) => {
            mp.0.iter()
                .filter_map(|p| p.exterior().centroid().map(|c| (c.x(), c.y())))
                .collect()
        }
        Rect(r) => vec![((r.min().x + r.max().x) / 2.0, (r.min().y + r.max().y) / 2.0)],
        Triangle(t) => {
            let v1 = t.v1();
            let v2 = t.v2();
            let v3 = t.v3();
            vec![((v1.x + v2.x + v3.x) / 3.0, (v1.y + v2.y + v3.y) / 3.0)]
        }
        GeometryCollection(gc) => gc.0.iter().flat_map(|g| geometry_anchors(g)).collect(),
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
