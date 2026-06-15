use thiserror::Error;

/// Unified error type covering all failure modes in the Geello library.
///
/// ## Variant Categories
///
/// | Category | Variants | Gate |
/// |----------|----------|------|
/// | I/O & Parsing | `Io`, `TomlDe`, `TomlSer` | — |
/// | Data Formats | `Json`, `GeoJson`, `Tiff` | `from_json`, `raster` |
/// | Network | `Http` | `from_http` |
/// | GPU & Rendering | `Wgpu`, `Render` | — |
/// | Server | `Image`, `Config`, `Cache` | `server` |
/// | Geometry | `Projection` | — |
/// | User Input | `InvalidArgument` | — |
#[derive(Error, Debug)]
pub enum GeelloError {
    /// File I/O errors (reading, writing, directory traversal).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing and serialization errors.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML deserialization errors (style files, config).
    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    /// TOML serialization errors.
    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// HTTP request / response errors (remote data loading).
    #[cfg(feature = "from_http")]
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// GeoJSON semantic errors beyond raw JSON parsing
    /// (e.g. unexpected geometry type, missing properties).
    #[error("GeoJSON error: {0}")]
    GeoJson(String),

    /// GeoTIFF decoding and processing errors.
    #[cfg(feature = "raster")]
    #[error("GeoTIFF error: {0}")]
    Tiff(String),

    /// Image encoding / decoding errors (PNG, JPEG, WebP).
    #[cfg(feature = "server")]
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    /// WGPU / GPU device initialization and operation errors.
    #[error("GPU error: {0}")]
    Wgpu(String),

    /// Coordinate projection / transformation errors.
    #[error("Projection error: {0}")]
    Projection(String),

    /// Figment-based configuration loading errors.
    #[cfg(feature = "server")]
    #[error("Configuration error: {0}")]
    Config(#[from] figment::Error),

    /// Cache system errors (memory / disk cache).
    #[error("Cache error: {0}")]
    Cache(String),

    /// Generic rendering pipeline errors.
    #[error("Render error: {0}")]
    Render(String),

    /// Invalid user-supplied arguments or parameters.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}

/// Convenience alias for results returned by Geello public APIs.
pub type GeelloResult<T> = Result<T, GeelloError>;
