use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub tmdb_id: u32,
    pub media_type: String,
}

pub struct CacheMetadataManager {
    file_path: PathBuf,
    mappings: Mutex<HashMap<String, CacheMetadata>>,
}

impl CacheMetadataManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("cache_metadata.json");

        let mappings = fs::read_to_string(&file_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();

        CacheMetadataManager {
            file_path,
            mappings: Mutex::new(mappings),
        }
    }

    fn save(&self, mappings: &HashMap<String, CacheMetadata>) -> Result<(), String> {
        let content = serde_json::to_string_pretty(mappings)
            .map_err(|e| format!("failed to serialize cache metadata: {}", e))?;
        fs::write(&self.file_path, content)
            .map_err(|e| format!("failed to write cache metadata: {}", e))?;
        Ok(())
    }

    pub fn set_mapping(&self, hash: String, tmdb_id: u32, media_type: String) -> Result<(), String> {
        let mut mappings = self.mappings.lock().map_err(|e| e.to_string())?;
        mappings.insert(hash.to_lowercase(), CacheMetadata {
            tmdb_id,
            media_type,
        });
        self.save(&mappings)
    }

    pub fn get_mapping(&self, hash: &str) -> Option<CacheMetadata> {
        self.mappings.lock().ok()?.get(&hash.to_lowercase()).cloned()
    }

    pub fn all(&self) -> HashMap<String, CacheMetadata> {
        self.mappings.lock().map(|m| m.clone()).unwrap_or_default()
    }

    pub fn remove_mapping(&self, hash: &str) -> Result<(), String> {
        let mut mappings = self.mappings.lock().map_err(|e| e.to_string())?;
        mappings.remove(&hash.to_lowercase());
        self.save(&mappings)
    }
}
