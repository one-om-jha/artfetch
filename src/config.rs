use std::path::PathBuf;
use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

pub const FIVE_K: (u32, u32) = (5120, 2880);
pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub download_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            download_dir: dirs::picture_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                .join("artfetch"),
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("artfetch")
        .join("config.json")
}

pub fn load_config() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(config: &Config) {
    let p = config_path();
    let _ = fs::create_dir_all(p.parent().unwrap());
    let _ = fs::write(p, serde_json::to_string_pretty(config).unwrap_or_default());
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("artfetch")
}

pub fn file_is_valid(path: &Path) -> bool {
    path.exists() && fs::metadata(path).map(|m| m.len() > 1000).unwrap_or(false)
}
