use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub last_season_number: Option<u32>,
    #[serde(default)]
    pub last_season_episode_count: Option<u32>,
}

impl WatchHistoryItem {
    /// builds a history entry from tmdb details (or a stored history item) and
    /// the position just recorded for it.
    pub fn from_media(
        media_id: u32,
        media_type: &str,
        media: &Value,
        position: Option<(Option<u32>, Option<u32>, f64)>,
        watched_at: i64,
    ) -> Self {
        let text = |key: &str| {
            media
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let number = |key: &str| media.get(key).and_then(|v| v.as_u64()).map(|n| n as u32);

        let seasons: Vec<&Value> = media
            .get("seasons")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter(|s| s.get("season_number").and_then(|n| n.as_u64()).unwrap_or(0) > 0)
                    .collect()
            })
            .unwrap_or_default();
        let last_season = seasons.last();
        let season_count = (!seasons.is_empty()).then_some(seasons.len() as u32);

        let (current_season, current_episode, timestamp) = position.unwrap_or((None, None, 0.0));

        Self {
            id: media_id,
            media_type: media_type.to_string(),
            title: text("title").or_else(|| text("name")).unwrap_or_else(|| "Unknown".to_string()),
            poster_path: text("poster_path"),
            backdrop_path: text("backdrop_path"),
            release_date: text("release_date").or_else(|| text("first_air_date")),
            vote_average: media
                .get("vote_average")
                .and_then(|v| v.as_f64())
                .filter(|v| *v != 0.0)
                .map(|v| v as f32),
            watched_at,
            current_season: current_season.filter(|s| *s > 0),
            current_episode: current_episode.filter(|e| *e > 0),
            current_timestamp: (timestamp > 0.0).then_some(timestamp),
            number_of_seasons: number("number_of_seasons").or(season_count),
            // a stored history item carries these directly instead of a season list
            last_season_number: last_season
                .and_then(|s| s.get("season_number"))
                .and_then(|n| n.as_u64())
                .map(|n| n as u32)
                .or_else(|| number("last_season_number")),
            last_season_episode_count: last_season
                .and_then(|s| s.get("episode_count"))
                .and_then(|n| n.as_u64())
                .map(|n| n as u32)
                .or_else(|| number("last_season_episode_count")),
        }
    }
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

    pub async fn add_item(&self, mut item: WatchHistoryItem) {
        let mut data = self.data.write().await;

        // quick play only has the stored history item, so keep metadata the new entry lacks
        if let Some(existing) = data
            .items
            .iter()
            .find(|existing| existing.id == item.id && existing.media_type == item.media_type)
        {
            item.poster_path = item.poster_path.or_else(|| existing.poster_path.clone());
            item.backdrop_path = item.backdrop_path.or_else(|| existing.backdrop_path.clone());
            item.release_date = item.release_date.or_else(|| existing.release_date.clone());
            item.vote_average = item.vote_average.or(existing.vote_average);
            item.number_of_seasons = item.number_of_seasons.or(existing.number_of_seasons);
            item.last_season_number = item.last_season_number.or(existing.last_season_number);
            item.last_season_episode_count = item
                .last_season_episode_count
                .or(existing.last_season_episode_count);
        }

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
