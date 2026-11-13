#![cfg(feature = "config-toml")]
use geello::config::*;
use geello::*;
use std::sync::Arc;
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 32 << 20,
            max_work: 100000,
            max_commands: 1000,
            max_diagnostics: 8,
        },
        None,
    )
}
const STYLE: &str = r##"
schema_version=1
id="demo:roads"
[[rules]]
id="roads"
filter={op="exists",field="/width"}
[[rules.symbols]]
id="casing"
kind="line"
width={value=6.0,unit="px"}
color="#FF0000"
[[rules.symbols]]
id="interior"
kind="line"
width={field="/width",fallback=2.0,unit="px"}
color="#0000FF"
"##;
#[test]
fn strict_schema_units_colors_and_relative_paths() {
    let style = parse_style(STYLE).unwrap();
    assert_eq!(style.rules[0].symbols.len(), 2);
    assert!(
        parse_style(&STYLE.replace("width={value=6.0", "unexpected=true\nwidth={value=6.0"))
            .is_err()
    );
    let data=parse_dataset("schema_version=1\nid='demo:stations'\nsource={kind='geojson',path='../data/stations.json'}","/app/config/dataset.toml").unwrap();
    assert_eq!(
        data.source.path,
        std::path::PathBuf::from("/app/config/../data/stations.json")
    );
    assert!(parse_style(&STYLE.replace("id=\"interior\"", "id=\"casing\"")).is_err());
    assert!(parse_color("#GG0000").is_err());
    assert_eq!(
        parse_color("#12345680").unwrap(),
        Color([0x12, 0x34, 0x56, 0x80])
    );
    assert!(
        parse_style(&STYLE.replace("width={value=6.0", "width={value=6.0,unit='bad'")).is_err()
    );
    let expression: NumericExpression = parse("field='/missing'\nfallback=3\nunit='map'").unwrap();
    assert_eq!(
        expression.length(&Properties::new()).unwrap(),
        Length::MapUnits(3.)
    );
}
#[test]
fn resource_graph_rejects_broken_map_and_animation_targets() {
    let mut graph = ResourceGraph::default();
    let map=parse_map("schema_version=1\nid='demo:map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='roads'\ndataset='demo:dataset'\nstyle='demo:roads'").unwrap();
    graph.maps.insert(map.id.clone(), map);
    assert!(graph.validate().is_err());
    let dataset = parse_dataset(
        "id='demo:dataset'\nsource={kind='geojson',path='data.json'}",
        "catalog.toml",
    )
    .unwrap();
    graph.datasets.insert(dataset.id.clone(), dataset);
    assert!(graph.validate().is_err());
    let style = parse_style(STYLE).unwrap();
    graph.styles.insert(style.id.clone(), style);
    graph.validate().unwrap();
    assert!(parse_animation("schema_version=1\nid='demo:pulse'\nduration=2.0\n[[tracks]]\nid='size'\nparameter='size'\ninterpolation='linear'\nkeyframes=[{time=1.0,value=2.0},{time=0.0,value=3.0}]").is_err());
}
#[test]
fn external_registration_removal_never_deletes_content() {
    let path =
        std::env::temp_dir().join(format!("geello-registration-{}.toml", std::process::id()));
    std::fs::write(&path, STYLE).unwrap();
    let mut registry = ExternalRegistry::default();
    registry.register("style", "demo:roads", &path).unwrap();
    assert!(
        registry
            .get("style", "demo:roads")
            .unwrap()
            .path
            .is_absolute()
    );
    assert!(registry.register("style", "demo:roads", &path).is_err());
    assert!(registry.remove("style", "demo:roads").is_some());
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
}
#[test]
fn resource_graph_checks_resource_keys_and_dataset_symbol_compatibility() {
    let dataset = parse_dataset(
        "id='demo:dataset'\nsource={kind='geotiff',path='data.tif'}",
        "catalog.toml",
    )
    .unwrap();
    let style = parse_style(
        "schema_version=1\nid='demo:style'\n[[rules]]\nid='all'\n[[rules.symbols]]\nid='image'\nkind='raster'\nmode='gray'\nband=1\nrange={kind='explicit',min=0,max=255}\nresampling={enlarge='nearest',reduce='nearest'}",
    )
    .unwrap();
    let map = parse_map(
        "schema_version=1\nid='demo:map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='image'\ndataset='demo:dataset'\nstyle='demo:style'",
    )
    .unwrap();
    let mut graph = ResourceGraph::default();
    graph.datasets.insert(dataset.id.clone(), dataset);
    graph.styles.insert(style.id.clone(), style);
    graph.maps.insert(map.id.clone(), map);
    graph.validate().unwrap();
    graph.datasets.get_mut("demo:dataset").unwrap().source.kind = "geojson".into();
    assert!(graph.validate().is_err());
    graph.datasets.get_mut("demo:dataset").unwrap().source.kind = "geotiff".into();
    graph
        .styles
        .insert("demo:style".into(), parse_style(STYLE).unwrap());
    assert!(graph.validate().is_err());
    graph.styles.get_mut("demo:style").unwrap().id = "demo:style".into();
    assert!(graph.validate().is_err());
}
#[cfg(feature = "render-cpu")]
#[test]
fn rules_are_symbol_major_across_batches_and_field_values() {
    let ctx = context();
    let crs = Crs::new("local", CoordinateSpace::Planar).unwrap();
    let mut batches = vec![];
    for (i, points) in [vec![(1., 16.), (30., 16.)], vec![(16., 1.), (16., 30.)]]
        .into_iter()
        .enumerate()
    {
        batches.push(Arc::new(VectorBatch {
            crs: crs.clone(),
            features: vec![Feature {
                source_id: None,
                id: Some(FeatureId::U64(i as u64)),
                geometry: Some(geo_types::LineString::from(points).into()),
                properties: std::collections::BTreeMap::from([(
                    "width".into(),
                    PropertyValue::F64(2.),
                )]),
                source_ordinal: i as u64,
                source_ordinates: vec![],
            }],
        }));
    }
    let items = resolve_vector_style(&parse_style(STYLE).unwrap(), &batches, &ctx).unwrap();
    let view = View {
        crs,
        bounds: Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 32.,
            max_y: 32.,
        },
        width: 32,
        height: 32,
        aspect: AspectPolicy::Stretch,
    };
    let image = CpuRenderer::new(0)
        .render(&prepare_scene(&items, &view, &ctx).unwrap(), &ctx)
        .unwrap();
    assert_eq!(
        &image.pixels()[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
        &[0, 0, 255, 255]
    );
}
