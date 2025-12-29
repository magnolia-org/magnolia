use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPreference {
    pub audio_track_index: Option<usize>,
    pub subtitle_track_index: Option<i32>,
    pub subtitle_language: Option<String>,
    pub subtitle_offset: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreferencesData {
    pub torrents: HashMap<String, TrackPreference>,
}

pub struct TrackPreferencesManager {
    file_path: PathBuf,
    data: Arc<RwLock<PreferencesData>>,
}

impl TrackPreferencesManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("track_preferences.json");
        let data = if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            PreferencesData::default()
        };

        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub async fn save_preference(
        &self,
        magnet_link: String,
        audio_track_index: Option<usize>,
        subtitle_track_index: Option<i32>,
        subtitle_language: Option<String>,
        subtitle_offset: Option<f64>,
    ) {
        let mut data = self.data.write().await;
        
        data.torrents.insert(magnet_link, TrackPreference {
            audio_track_index,
            subtitle_track_index,
            subtitle_language,
            subtitle_offset,
        });

        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }

    pub async fn get_preference(&self, magnet_link: &str) -> Option<TrackPreference> {
        let data = self.data.read().await;
        data.torrents.get(magnet_link).cloned()
    }
}
