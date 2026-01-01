use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub tmdb_id: u32,
    pub media_type: String,
}

pub struct CacheMetadataManager {
    file_path: PathBuf,
    pub mappings: HashMap<String, CacheMetadata>,
}

impl CacheMetadataManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self, String> {
        let file_path = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("failed to get app data dir: {}", e))?
            .join("cache_metadata.json");
        
        let mappings = if file_path.exists() {
            let content = fs::read_to_string(&file_path)
                .map_err(|e| format!("failed to read cache metadata: {}", e))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };
        
        Ok(CacheMetadataManager {
            file_path,
            mappings,
        })
    }
    
    fn save(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.mappings)
            .map_err(|e| format!("failed to serialize cache metadata: {}", e))?;
        fs::write(&self.file_path, content)
            .map_err(|e| format!("failed to write cache metadata: {}", e))?;
        Ok(())
    }
    
    pub fn set_mapping(&mut self, hash: String, tmdb_id: u32, media_type: String) -> Result<(), String> {
        self.mappings.insert(hash.to_lowercase(), CacheMetadata {
            tmdb_id,
            media_type,
        });
        self.save()
    }
    
    pub fn get_mapping(&self, hash: &str) -> Option<CacheMetadata> {
        self.mappings.get(&hash.to_lowercase()).cloned()
    }
    
    #[allow(dead_code)]
    pub fn remove_mapping(&mut self, hash: &str) -> Result<(), String> {
        self.mappings.remove(&hash.to_lowercase());
        self.save()
    }
}

#[tauri::command]
pub fn save_cache_metadata(
    hash: String,
    tmdb_id: u32,
    media_type: String,
    manager: tauri::State<std::sync::Mutex<CacheMetadataManager>>,
) -> Result<(), String> {
    let mut mgr = manager.lock().unwrap();
    mgr.set_mapping(hash, tmdb_id, media_type)
}

#[tauri::command]
pub fn get_cache_metadata(
    hash: String,
    manager: tauri::State<std::sync::Mutex<CacheMetadataManager>>,
) -> Result<Option<CacheMetadata>, String> {
    let mgr = manager.lock().unwrap();
    Ok(mgr.get_mapping(&hash))
}

#[tauri::command]
pub fn get_all_cache_metadata(
    manager: tauri::State<std::sync::Mutex<CacheMetadataManager>>,
) -> Result<HashMap<String, CacheMetadata>, String> {
    let mgr = manager.lock().unwrap();
    Ok(mgr.mappings.clone())
}
