//! Geello's memory rendering API and optional layer-two readers.
//!
//! Scene preparation and execution own memory only. Optional readers and TOML
//! loading perform explicit, budgeted I/O before preparing an immutable scene.

pub mod animation;
#[cfg(any(feature = "render-cpu", feature = "render-gpu"))]
mod backend;
#[cfg(feature = "config-toml")]
pub mod config;
mod core;
#[cfg(feature = "derived-store")]
pub mod derived;
#[cfg(feature = "encode-images")]
pub mod encoding;
mod operation;
#[cfg(feature = "rust-proj")]
pub mod projection;
pub mod query;
mod raster;
pub mod raster_processing;
#[cfg(any(
    feature = "read-geojson",
    feature = "read-shapefile",
    feature = "read-geotiff",
    feature = "derived-store"
))]
pub mod readers;
#[cfg(feature = "render-cpu")]
mod render_cpu;
#[cfg(feature = "render-gpu")]
mod render_gpu;
mod scene;
#[cfg(feature = "server")]
pub mod server;
mod style;
#[cfg(feature = "text")]
pub mod text;

pub use core::*;
pub use operation::*;
pub use raster::*;
#[cfg(feature = "render-cpu")]
pub use render_cpu::*;
#[cfg(feature = "render-gpu")]
pub use render_gpu::*;
pub use scene::*;
pub use style::*;
