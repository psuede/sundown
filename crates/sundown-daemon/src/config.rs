use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_DIR: &str = "/etc/sundown";
const DEFAULT_PORT: u16 = 48800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub timekpr: TimekprConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub token_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimekprConfig {
    pub user: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                bind: "0.0.0.0".to_string(),
                port: DEFAULT_PORT,
            },
            auth: AuthConfig {
                token_file: PathBuf::from(DEFAULT_CONFIG_DIR).join("token"),
            },
            timekpr: TimekprConfig {
                user: "kid".to_string(),
            },
        }
    }
}

impl Config {
    /// Create a default config with paths relative to the given config file location.
    pub fn default_for(config_path: &Path) -> Self {
        let dir = config_path
            .parent()
            .unwrap_or(Path::new(DEFAULT_CONFIG_DIR));
        Self {
            auth: AuthConfig {
                token_file: dir.join("token"),
            },
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.bind, self.server.port)
    }
}
