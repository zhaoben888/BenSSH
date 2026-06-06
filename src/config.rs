use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VpsEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    // 保持与原有 TS 版本的驼峰命名（keyPath）向后兼容
    #[serde(rename = "keyPath")]
    pub key_path: Option<String>,
    pub setup: Option<bool>,
}

fn config_dir() -> Option<PathBuf> {
    let mut path = dirs::home_dir()?;
    path.push(".vps-manager");
    Some(path)
}

fn config_path() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("config.json");
    Some(path)
}

pub fn load_config() -> Result<Vec<VpsEntry>, Box<dyn std::error::Error>> {
    let path = config_path().ok_or("找不到系统 Home 目录")?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let data = fs::read_to_string(path)?;
    let entries: Vec<VpsEntry> = serde_json::from_str(&data)?;
    Ok(entries)
}

pub fn save_config(entries: &[VpsEntry]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = config_dir().ok_or("找不到系统 Home 目录")?;
    fs::create_dir_all(&dir)?;

    let path = config_path().unwrap();
    let data = serde_json::to_string_pretty(entries)?;
    fs::write(path, data)?;

    Ok(())
}
