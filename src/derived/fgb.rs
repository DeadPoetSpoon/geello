//! In-memory FlatGeobuf encoding: no upstream writer or hidden disk spool.
//! The caller admits a conservative memory envelope before entering this module.
use super::error;
use crate::{Feature, GeelloError, OperationContext};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use flatgeobuf::packed_r_tree::{NodeItem, PackedRTree, calc_extent, hilbert_sort};
use flatgeobuf::{
    Column, ColumnArgs, ColumnType, FeatureArgs, GeometryArgs, GeometryType, Header, HeaderArgs,
};
use geo::BoundingRect;
use geo_types::Geometry;

/// Each source allocation contributes at most sixteen times its native size to
/// coordinates, table/vector alignment and nested geometry tables. The fixed
/// allowance includes the feature, property vector and header/index overhead.
pub(super) fn envelope(feature: &Feature) -> Result<usize, GeelloError> {
    crate::core::feature_clone_bytes(feature)?
        .checked_mul(16)
        .and_then(|n| n.checked_add(4096))
        .ok_or(GeelloError::SizeOverflow)
}

pub(super) fn encode(
    features: &[Feature],
    max: usize,
    ctx: &OperationContext,
) -> Result<Option<Vec<u8>>, GeelloError> {
    let mut nodes = Vec::with_capacity(features.len());
    let mut records = Vec::with_capacity(features.len());
    let mut length = 0usize;
    for feature in features {
        ctx.checkpoint(1)?;
        let Some(g) = feature.geometry.as_ref() else {
            continue;
        };
        let Some(b) = g.bounding_rect() else { continue };
        let capacity = envelope(feature)?;
        if capacity > flatbuffers::FLATBUFFERS_MAX_BUFFER_SIZE {
            return Err(GeelloError::BudgetExceeded("FlatBuffers feature capacity"));
        }
        // Preallocation is deliberately larger than the encoded payload. Geometry
        // vectors and table offsets are also covered by the caller's envelope.
        let mut builder = FlatBufferBuilder::with_capacity(capacity);
        let geometry = geometry(&mut builder, g)?;
        let mut properties = [0u8; 10];
        properties[2..].copy_from_slice(&feature.source_ordinal.to_le_bytes());
        let properties = builder.create_vector(&properties);
        let value = flatgeobuf::Feature::create(
            &mut builder,
            &FeatureArgs {
                geometry: Some(geometry),
                properties: Some(properties),
                ..Default::default()
            },
        );
        builder.finish_size_prefixed(value, None);
        let bytes = builder.finished_data();
        length = length
            .checked_add(bytes.len())
            .ok_or(GeelloError::SizeOverflow)?;
        if length > max {
            return Err(GeelloError::BudgetExceeded("FGB shard"));
        }
        records.push(bytes.to_vec());
        let mut node = NodeItem::bounds(b.min().x, b.min().y, b.max().x, b.max().y);
        node.offset = (records.len() - 1) as u64;
        nodes.push(node);
        // This is an internal invariant, not an acceptance of unchecked growth.
        debug_assert_eq!(builder.collapse().0.len(), capacity);
    }
    if nodes.is_empty() {
        return Ok(None);
    }
    let extent = calc_extent(&nodes);
    let mut b = FlatBufferBuilder::with_capacity(1024);
    let name = b.create_string("geello-xy");
    let column_name = b.create_string("ordinal");
    let col = Column::create(
        &mut b,
        &ColumnArgs {
            name: Some(column_name),
            type_: ColumnType::ULong,
            nullable: false,
            ..Default::default()
        },
    );
    let columns = b.create_vector(&[col]);
    let bounds = b.create_vector(&[extent.min_x, extent.min_y, extent.max_x, extent.max_y]);
    let header = Header::create(
        &mut b,
        &HeaderArgs {
            name: Some(name),
            columns: Some(columns),
            envelope: Some(bounds),
            features_count: nodes.len() as u64,
            ..Default::default()
        },
    );
    b.finish_size_prefixed(header, None);
    let index_size = PackedRTree::index_size(nodes.len(), PackedRTree::DEFAULT_NODE_SIZE);
    let total = length
        .checked_add(index_size)
        .and_then(|n| n.checked_add(8 + b.finished_data().len()))
        .ok_or(GeelloError::SizeOverflow)?;
    if total > max {
        return Err(GeelloError::BudgetExceeded("FGB shard"));
    }
    hilbert_sort(&mut nodes, &extent);
    let mut offset = 0;
    let index_nodes: Vec<_> = nodes
        .iter()
        .map(|n| {
            let mut node = n.clone();
            node.offset = offset;
            offset += records[n.offset as usize].len() as u64;
            node
        })
        .collect();
    let tree =
        PackedRTree::build(&index_nodes, &extent, PackedRTree::DEFAULT_NODE_SIZE).map_err(error)?;
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(&[b'f', b'g', b'b', 3, b'f', b'g', b'b', 0]);
    output.extend_from_slice(b.finished_data());
    tree.stream_write(&mut output).map_err(error)?;
    for node in nodes {
        output.extend_from_slice(&records[node.offset as usize]);
    }
    debug_assert_eq!(output.len(), total);
    Ok(Some(output))
}

fn geometry<'a>(
    b: &mut FlatBufferBuilder<'a>,
    g: &Geometry<f64>,
) -> Result<WIPOffset<flatgeobuf::Geometry<'a>>, GeelloError> {
    use geo_types::Geometry::*;
    let mut xy = Vec::new();
    let mut ends = Vec::new();
    let mut parts = Vec::new();
    let mut line = |l: &geo_types::LineString<f64>| -> Result<(), GeelloError> {
        for c in &l.0 {
            xy.extend_from_slice(&[c.x, c.y]);
        }
        ends.push(u32::try_from(xy.len() / 2).map_err(|_| GeelloError::SizeOverflow)?);
        Ok(())
    };
    let kind = match g {
        Point(p) => {
            xy.extend_from_slice(&[p.x(), p.y()]);
            GeometryType::Point
        }
        MultiPoint(p) => {
            for p in &p.0 {
                xy.extend_from_slice(&[p.x(), p.y()]);
            }
            GeometryType::MultiPoint
        }
        LineString(l) => {
            line(l)?;
            GeometryType::LineString
        }
        MultiLineString(m) => {
            for l in &m.0 {
                line(l)?;
            }
            GeometryType::MultiLineString
        }
        Polygon(p) => {
            line(p.exterior())?;
            for l in p.interiors() {
                line(l)?;
            }
            GeometryType::Polygon
        }
        MultiPolygon(m) => {
            for p in &m.0 {
                parts.push(geometry(b, &Polygon(p.clone()))?);
            }
            GeometryType::MultiPolygon
        }
        GeometryCollection(m) => {
            for g in &m.0 {
                parts.push(geometry(b, g)?);
            }
            GeometryType::GeometryCollection
        }
        Line(l) => return geometry(b, &LineString(geo_types::LineString(vec![l.start, l.end]))),
        Rect(r) => return geometry(b, &Polygon(r.to_polygon())),
        Triangle(t) => return geometry(b, &Polygon(t.to_polygon())),
    };
    let xy = (!xy.is_empty()).then(|| b.create_vector(&xy));
    // Explicit zero ends retain empty components within a multi-geometry.
    let ends = (!ends.is_empty()).then(|| b.create_vector(&ends));
    let parts = (!parts.is_empty()).then(|| b.create_vector(&parts));
    Ok(flatgeobuf::Geometry::create(
        b,
        &GeometryArgs {
            xy,
            ends,
            parts,
            type_: kind,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flatgeobuf::geozero::GeomProcessor;
    use flatgeobuf::{FallibleStreamingIterator, FeatureProperties, FgbReader, GeozeroGeometry};
    use geo::CoordsIter;
    use geo_types::{
        GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Polygon,
        line_string, point,
    };
    #[derive(Default)]
    struct Coordinates(Vec<[f64; 2]>);
    impl GeomProcessor for Coordinates {
        fn xy(&mut self, x: f64, y: f64, _: usize) -> flatgeobuf::geozero::error::Result<()> {
            self.0.push([x, y]);
            Ok(())
        }
    }
    #[test]
    fn full_xy_geometries_survive_hilbert_order_and_nested_parts() {
        let polygon = Polygon::new(
            line_string![(x:0.,y:0.),(x:8.,y:0.),(x:8.,y:8.),(x:0.,y:0.)],
            vec![line_string![(x:1.,y:1.),(x:2.,y:1.),(x:1.,y:2.),(x:1.,y:1.)]],
        );
        let geometries: Vec<Geometry<f64>> = vec![
            point!(x:100.,y:100.).into(),
            line_string![(x:20.,y:20.),(x:40.,y:40.)].into(),
            polygon.clone().into(),
            MultiPoint(vec![point!(x:-8.,y:-8.), point!(x:-7.,y:-6.)]).into(),
            MultiLineString(vec![
                LineString(vec![]),
                line_string![(x:4.,y:4.),(x:9.,y:9.)],
            ])
            .into(),
            MultiPolygon(vec![polygon.clone(), polygon]).into(),
            Geometry::GeometryCollection(GeometryCollection(vec![
                point!(x:3.,y:4.).into(),
                Geometry::GeometryCollection(GeometryCollection(vec![
                    line_string![(x:1.,y:2.),(x:3.,y:4.)].into(),
                ])),
            ])),
        ];
        let features: Vec<_> = geometries
            .iter()
            .enumerate()
            .map(|(i, g)| Feature {
                geometry: Some(g.clone()),
                source_ordinal: i as u64,
                source_id: None,
                id: None,
                properties: Default::default(),
                source_ordinates: vec![],
            })
            .collect();
        let ctx = OperationContext::new(
            crate::OperationLimits {
                max_live_bytes: 16 << 20,
                max_work: 100000,
                max_commands: 1000,
                max_diagnostics: 8,
            },
            None,
        );
        let bytes = encode(&features, 1 << 20, &ctx).unwrap().unwrap();
        let mut input = std::io::Cursor::new(&bytes);
        let mut reader = FgbReader::open(&mut input).unwrap().select_all().unwrap();
        let mut seen = vec![];
        while let Some(f) = reader.next().unwrap() {
            let ordinal = f.property::<u64>("ordinal").unwrap() as usize;
            let mut coords = Coordinates::default();
            f.process_geom(&mut coords).unwrap();
            let expected: Vec<_> = geometries[ordinal]
                .coords_iter()
                .map(|c| [c.x, c.y])
                .collect();
            assert_eq!(coords.0, expected);
            seen.push(ordinal);
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..features.len()).collect::<Vec<_>>());
        assert!(encode(&features, bytes.len() - 1, &ctx).is_err());
    }
}
