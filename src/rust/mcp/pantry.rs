use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PantryLabel {
    pub dish_type: String,
    pub tag: Option<String>,
    pub size_bytes: u64,
}

pub fn pantry_base_dir() -> Result<PathBuf> {
    let root = dirs::cache_dir()
        .or_else(dirs::data_dir)
        .or_else(dirs::config_dir)
        .unwrap_or_else(std::env::temp_dir);
    let base = root.join("bistro").join("pantry");
    fs::create_dir_all(&base)
        .map_err(|e| anyhow::anyhow!("无法创建缓存目录 {}: {}", base.display(), e))?;
    Ok(base)
}

fn spice_dir(base: &Path, spice_id: &str) -> PathBuf {
    base.join(spice_id)
}

pub fn stash_ingredient_bytes(bytes: &[u8], dish_type: &str, tag: Option<String>) -> Result<String> {
    let base = pantry_base_dir()?;
    let spice_id = Uuid::new_v4().to_string();
    let dir = spice_dir(&base, &spice_id);
    fs::create_dir_all(&dir)?;

    fs::write(dir.join("ingredient.bin"), bytes)?;

    let label = PantryLabel {
        dish_type: dish_type.to_string(),
        tag,
        size_bytes: bytes.len() as u64,
    };
    fs::write(dir.join("label.json"), serde_json::to_string(&label)?)?;

    Ok(spice_id)
}

pub fn fetch_ingredient_bytes(spice_id: &str) -> Result<(Vec<u8>, PantryLabel)> {
    let base = pantry_base_dir()?;
    let dir = spice_dir(&base, spice_id);
    let label_str = fs::read_to_string(dir.join("label.json"))?;
    let label: PantryLabel = serde_json::from_str(&label_str)?;
    let bytes = fs::read(dir.join("ingredient.bin"))?;
    Ok((bytes, label))
}

pub fn discard_spice(spice_id: &str) -> Result<()> {
    let base = pantry_base_dir()?;
    let dir = spice_dir(&base, spice_id);
    if dir.exists() {
        let _ = fs::remove_dir_all(dir);
    }
    Ok(())
}

pub fn clean_expired_pantry_items(max_age: Duration) -> Result<usize> {
    let base = pantry_base_dir()?;
    let now = SystemTime::now();
    let mut deleted = 0usize;

    let entries = match fs::read_dir(&base) {
        Ok(v) => v,
        Err(_) => return Ok(0),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let label_path = path.join("label.json");
        let modified = fs::metadata(&label_path)
            .and_then(|m| m.modified())
            .or_else(|_| fs::metadata(&path).and_then(|m| m.modified()));

        let modified = match modified {
            Ok(t) => t,
            Err(_) => continue,
        };

        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age <= max_age {
            continue;
        }

        if fs::remove_dir_all(&path).is_ok() {
            deleted += 1;
        }
    }

    Ok(deleted)
}
