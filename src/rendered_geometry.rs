use crate::{RenderedGeometryFilter, TileProj};
use geo::{
    BooleanOps, BoundingRect, Centroid, Contains, ConvexHull, CoordsIter, Geometry, Intersects,
    LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
pub struct RenderedGeometry {
    layer: Option<String>,
    inner_geom: Geometry,
    center_point: Option<Point>,
    has_calc_center_point: bool,
    lines: Option<MultiLineString>,
    has_calc_lines: bool,
    areas: Option<MultiPolygon>,
    has_calc_areas: bool,
}

impl RenderedGeometry {
    pub fn new_temp(inner_geom: Geometry) -> Self {
        RenderedGeometry::new(None, inner_geom, &None)
    }
    pub fn new(layer: Option<String>, mut inner_geom: Geometry, proj: &Option<TileProj>) -> Self {
        if let Some(proj) = proj {
            crate::utils::transform(&mut inner_geom, proj);
        }
        RenderedGeometry {
            layer,
            inner_geom,
            center_point: None,
            has_calc_center_point: false,
            lines: None,
            has_calc_lines: false,
            areas: None,
            has_calc_areas: false,
        }
    }
    pub fn fit_filter(&self, filter: &RenderedGeometryFilter) -> bool {
        match filter {
            RenderedGeometryFilter::None => true,
            RenderedGeometryFilter::Layer(other_layer) => {
                if let Some(self_layer) = &self.layer {
                    self_layer == other_layer
                } else {
                    true
                }
            }
        }
    }
    pub fn lines(&mut self) -> Option<&MultiLineString> {
        if self.has_calc_lines {
            return self.lines.as_ref();
        }
        let lines = RenderedGeometry::get_lines_from_geom(&self.inner_geom);
        self.lines = if lines.len() > 0 {
            let multi = MultiLineString::new(lines);
            Some(multi)
        } else {
            None
        };
        self.has_calc_lines = true;
        self.lines.as_ref()
    }
    pub fn areas(&mut self) -> Option<&MultiPolygon> {
        if self.has_calc_areas {
            return self.areas.as_ref();
        }
        let areas = RenderedGeometry::get_areas_from_geom(&self.inner_geom);
        self.areas = if areas.len() > 0 {
            let multi = MultiPolygon::new(areas);
            Some(multi)
        } else {
            None
        };
        self.has_calc_areas = true;
        self.areas.as_ref()
    }
    fn get_lines_from_geom(geom: &Geometry) -> Vec<LineString> {
        let mut lines = Vec::new();
        match geom {
            Geometry::Point(_) => {}
            Geometry::Line(line) => {
                let line_string = LineString::from_iter(line.coords_iter());
                lines.push(line_string);
            }
            Geometry::LineString(line_string) => {
                lines.push(line_string.clone());
            }
            Geometry::Polygon(polygon) => {
                lines.push(polygon.exterior().clone());
                for ele in polygon.interiors() {
                    lines.push(ele.clone());
                }
            }
            Geometry::MultiPoint(multi_point) => {
                let line_string = LineString::from_iter(multi_point.coords_iter());
                lines.push(line_string);
            }
            Geometry::MultiLineString(multi_line_string) => {
                for line_string in multi_line_string {
                    lines.push(line_string.clone());
                }
            }
            Geometry::MultiPolygon(multi_polygon) => {
                for polygon in multi_polygon {
                    lines.push(polygon.exterior().clone());
                    for ele in polygon.interiors() {
                        lines.push(ele.clone());
                    }
                }
            }
            Geometry::GeometryCollection(geometry_collection) => {
                for ele in geometry_collection {
                    let mut ele_lines = RenderedGeometry::get_lines_from_geom(ele);
                    lines.append(&mut ele_lines);
                }
            }
            Geometry::Rect(rect) => {
                let mut line_string = LineString::from_iter(rect.coords_iter());
                line_string.close();
                lines.push(line_string);
            }
            Geometry::Triangle(triangle) => {
                let mut line_string = LineString::from_iter(triangle.coords_iter());
                line_string.close();
                lines.push(line_string);
            }
        };
        lines
    }
    fn get_areas_from_geom(geom: &Geometry) -> Vec<Polygon> {
        let mut polygons = Vec::new();
        match geom {
            Geometry::Point(_) => {}
            Geometry::Line(line) => {
                polygons.push(line.bounding_rect().to_polygon());
            }
            Geometry::LineString(line_string) => {
                polygons.push(line_string.convex_hull());
            }
            Geometry::Polygon(polygon) => {
                polygons.push(polygon.clone());
            }
            Geometry::MultiPoint(multi_point) => {
                polygons.push(multi_point.convex_hull());
            }
            Geometry::MultiLineString(multi_line_string) => {
                polygons.push(multi_line_string.convex_hull());
            }
            Geometry::MultiPolygon(multi_polygon) => {
                for polygon in multi_polygon {
                    polygons.push(polygon.clone());
                }
            }
            Geometry::GeometryCollection(geometry_collection) => {
                for ele in geometry_collection {
                    let mut ele_lines = RenderedGeometry::get_areas_from_geom(ele);
                    polygons.append(&mut ele_lines);
                }
            }
            Geometry::Rect(rect) => {
                polygons.push(rect.to_polygon());
            }
            Geometry::Triangle(triangle) => {
                polygons.push(triangle.to_polygon());
            }
        };
        polygons
    }
    pub fn center_point(&mut self, rect: Option<Rect>) -> Option<&Point> {
        if rect.is_none() && self.has_calc_center_point {
            self.center_point.as_ref()
        } else {
            let center = match rect {
                Some(rect) => match &self.inner_geom {
                    Geometry::Point(point) => Some(point.clone()),
                    Geometry::Line(line) => {
                        if rect.intersects(&line.start) {
                            Some(line.start_point())
                        } else {
                            if rect.intersects(&line.end) {
                                Some(line.end_point())
                            } else {
                                None
                            }
                        }
                    }
                    Geometry::LineString(line_string) => {
                        let multi_line = MultiLineString::new(vec![line_string.clone()]);
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.clip(&multi_line, false);
                        intersection.centroid()
                    }
                    Geometry::Polygon(polygon) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.intersection(polygon);
                        intersection.centroid()
                    }
                    Geometry::MultiPoint(multi_point) => {
                        let mut inter_point = Vec::new();
                        for point in multi_point {
                            if rect.contains(point) {
                                inter_point.push(point.clone());
                            }
                        }
                        let multi_point = MultiPoint::new(inter_point);
                        multi_point.centroid()
                    }
                    Geometry::MultiLineString(multi_line_string) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.clip(&multi_line_string, false);
                        intersection.centroid()
                    }
                    Geometry::MultiPolygon(multi_polygon) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.intersection(multi_polygon);
                        intersection.centroid()
                    }
                    Geometry::GeometryCollection(geometry_collection) => {
                        let mut inter_point = Vec::new();
                        for geom in geometry_collection {
                            let point = geom.centroid().unwrap_or_default();
                            if rect.contains(&point) {
                                inter_point.push(point);
                            }
                        }
                        let multi_point = MultiPoint::new(inter_point);
                        multi_point.centroid()
                    }
                    Geometry::Rect(o_rect) => {
                        let rect_polygon = rect.to_polygon();
                        let o_rect_polygon = o_rect.to_polygon();
                        let intersection = rect_polygon.intersection(&o_rect_polygon);
                        intersection.centroid()
                    }
                    Geometry::Triangle(triangle) => {
                        let rect_polygon = rect.to_polygon();
                        let triangle_polygon = triangle.to_polygon();
                        let intersection = rect_polygon.intersection(&triangle_polygon);
                        intersection.centroid()
                    }
                },
                None => self.inner_geom.centroid(),
            };
            if rect.is_none() {
                self.has_calc_center_point = true;
            }
            self.center_point = center;
            self.center_point.as_ref()
        }
    }
}
