use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchHistoryItem {
    pub id: u32,
    pub media_type: String,
    pub title: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub vote_average: Option<f32>,
    pub watched_at: i64,
    pub current_season: Option<u32>,
    pub current_episode: Option<u32>,
    pub current_timestamp: Option<f64>,
    #[serde(default)]
    pub number_of_seasons: Option<u32>,
    #[serde(default)]
    pub last_season_episode_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchHistoryData {
    pub items: Vec<WatchHistoryItem>,
}

pub struct WatchHistoryManager {
    file_path: PathBuf,
    data: Arc<RwLock<WatchHistoryData>>,
}

impl WatchHistoryManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("watch_history.json");
        let data = if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            WatchHistoryData::default()
        };

        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub async fn add_item(&self, item: WatchHistoryItem) {
        let mut data = self.data.write().await;
        
        // Remove existing entry if present
        data.items.retain(|existing| 
            !(existing.id == item.id && existing.media_type == item.media_type)
        );
        
        // Add to front
        data.items.insert(0, item);
        
        // Keep only last 20 items
        data.items.truncate(20);
        
        // Persist to disk
        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }

    pub async fn get_history(&self) -> Vec<WatchHistoryItem> {
        let data = self.data.read().await;
        data.items.clone()
    }

    pub async fn remove_item(&self, media_id: u32, media_type: String) {
        let mut data = self.data.write().await;
        
        data.items.retain(|item| 
            !(item.id == media_id && item.media_type == media_type)
        );
        
        // Persist to disk
        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }

    pub async fn clear(&self) {
        let mut data = self.data.write().await;
        data.items.clear();
        
        // Persist to disk
        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }
}
