use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_address")]
    pub address: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_path")]
    pub data_path: PathBuf,
}

fn default_address() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))
}
fn default_port() -> u16 {
    8000
}
fn default_data_path() -> PathBuf {
    PathBuf::from("data")
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            data_path: default_data_path(),
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
            "Server config: address={}:{}, data_path={}",
            config.address,
            config.port,
            config.data_path.display()
        );

        Ok(config)
    }
}
