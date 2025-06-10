use clap::Parser;
use geello::{AreaRenderer, LineRenderer, PointRenderer, RenderOption, RenderRegion};
use ron::ser::PrettyConfig;
use std::path::PathBuf;

#[pollster::main]
async fn main() -> anyhow::Result<()> {
    // let mut option = RenderOption::default();
    // option.region = RenderRegion::TileIndex(47, 8, 5);
    // option
    //     .renderers
    //     .push(geello::GeometryRenderer::Area(AreaRenderer::default()));
    // option
    //     .renderers
    //     .push(geello::GeometryRenderer::Line(LineRenderer::default()));
    // option
    //     .renderers
    //     .push(geello::GeometryRenderer::Point(PointRenderer::default()));
    // let str = ron::ser::to_string_pretty(&option, PrettyConfig::default())?;
    // std::fs::write("assets/test/render_option.ron", str)?;
    env_logger::init();
    let cli = Cli::parse();
    geello::render_geojson_file_to_image_with_option_file(cli.geojson, cli.option, cli.dst).await?;
    Ok(())
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// geojson path
    #[arg(short, long, value_name = "FILE")]
    geojson: PathBuf,

    /// render option path
    #[arg(short, long, value_name = "FILE")]
    option: PathBuf,

    /// dest image path
    #[arg(short, long, value_name = "FILE")]
    dst: PathBuf,
}
