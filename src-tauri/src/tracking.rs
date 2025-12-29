use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeTorrent {
    pub magnet_link: String,
    pub file_index: usize, // The specific file index within the torrent
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonTorrent {
    // Map episode number to torrent info
    pub episodes: HashMap<u32, EpisodeTorrent>,
    // If a batch torrent covers the whole season, we might store it here
    // For now, we'll just map each episode individually, even if they share the same magnet link
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShowHistory {
    // Map season number to season info
    pub seasons: HashMap<u32, SeasonTorrent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryData {
    // Map show ID (TMDB ID) to history
    pub shows: HashMap<u32, ShowHistory>,
}

pub struct TrackingManager {
    file_path: PathBuf,
    data: Arc<RwLock<HistoryData>>,
}

impl TrackingManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("history.json");
        let data = if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HistoryData::default()
        };

        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub async fn save_selection(&self, show_id: u32, season: u32, episode: u32, magnet_link: String, file_index: usize) {
        let mut data = self.data.write().await;
        
        let show = data.shows.entry(show_id).or_default();
        let season_data = show.seasons.entry(season).or_insert_with(|| SeasonTorrent {
            episodes: HashMap::new(),
        });

        season_data.episodes.insert(episode, EpisodeTorrent {
            magnet_link,
            file_index,
        });

        // Persist to disk
        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }

    pub async fn get_selection(&self, show_id: u32, season: u32, episode: u32) -> Option<EpisodeTorrent> {
        let data = self.data.read().await;
        data.shows.get(&show_id)
            .and_then(|show| show.seasons.get(&season))
            .and_then(|season_data| season_data.episodes.get(&episode))
            .cloned()
    }

    pub async fn get_all_selections(&self, show_id: u32) -> Option<ShowHistory> {
        let data = self.data.read().await;
        data.shows.get(&show_id).cloned()
    }

    pub async fn remove_selection(&self, show_id: u32, season: u32, episode: u32) {
        let mut data = self.data.write().await;
        
        if let Some(show) = data.shows.get_mut(&show_id) {
            if let Some(season_data) = show.seasons.get_mut(&season) {
                season_data.episodes.remove(&episode);
            }
        }

        // Persist to disk
        if let Ok(content) = serde_json::to_string_pretty(&*data) {
            let _ = fs::write(&self.file_path, content);
        }
    }
}
