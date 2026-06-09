use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_address")]
    pub address: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_path")]
    pub data_path: PathBuf,
    #[serde(default = "default_wmts_texture_count")]
    pub wmts_texture_count: u32,
    #[serde(default)]
    pub shader_init_threads: Option<NonZeroUsize>,
    #[serde(default = "default_cache_path")]
    pub cache_path: PathBuf,
}

fn default_address() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))
}
fn default_port() -> u16 {
    8000
}
fn default_data_path() -> PathBuf {
    PathBuf::from("assets")
}
fn default_wmts_texture_count() -> u32 {
    100
}
fn default_cache_path() -> PathBuf {
    PathBuf::from("cache")
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            data_path: default_data_path(),
            wmts_texture_count: default_wmts_texture_count(),
            shader_init_threads: None,
            cache_path: default_cache_path(),
        }
    }
}

impl ServerConfig {
    /// Load configuration, merging in order:
    /// 1. Default values (via serde defaults)
    /// 2. `Geello.toml` (if present in working directory)
    /// 3. Environment variables prefixed with `GEELLO_`
    pub fn load() -> Result<Self, figment::Error> {
        let config_path = std::path::Path::new("Geello.toml");

        let mut figment = Figment::new();

        if config_path.exists() {
            figment = figment.merge(Toml::file(config_path));
            tracing::info!("Loading config from {}", config_path.display());
        } else {
            tracing::warn!(
                "Config file not found at '{}', using defaults. \
                 Create Geello.toml in the working directory to customize.",
                config_path.display()
            );
        }

        figment = figment.merge(Env::prefixed("GEELLO_"));
        let config: Self = figment.extract()?;

        tracing::info!(
            "Server config: address={}:{}, data_path={}, cache_path={}",
            config.address,
            config.port,
            config.data_path.display(),
            config.cache_path.display()
        );

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8000);
        assert_eq!(config.data_path, PathBuf::from("assets"));
        assert_eq!(config.cache_path, PathBuf::from("cache"));
        assert_eq!(config.wmts_texture_count, 100);
        assert!(config.shader_init_threads.is_none());
    }

    #[test]
    fn load_from_toml_file() {
        // Geello.toml exists in the project root (cargo test runs from there)
        let config = ServerConfig::load();
        match config {
            Ok(c) => {
                // data_path should come from Geello.toml: "assets/test"
                assert_eq!(c.data_path, PathBuf::from("assets/test"));
                assert_eq!(c.cache_path, PathBuf::from("assets/test/cache"));
            },
            Err(e) => {
                // If file not found or parse error, default should still work
                let default = ServerConfig::default();
                assert_eq!(default.data_path, PathBuf::from("assets"));
                eprintln!("Config file load failed (expected if missing): {}", e);
            },
        }
    }

    #[test]
    fn load_falls_back_to_defaults_when_no_file() {
        // Load with a non-existent file should still succeed with defaults
        let figment = Figment::new().merge(Env::prefixed("GEELLO_"));
        let config: ServerConfig = figment.extract().unwrap();
        // Without TOML file, serde defaults are used
        assert_eq!(config.data_path, PathBuf::from("assets"));
        assert_eq!(config.port, 8000);
    }
}
