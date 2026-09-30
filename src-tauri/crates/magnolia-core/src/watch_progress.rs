use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::watch_state::{episode_key, progress_key, with_watched_flag, ProgressEntry};

/// a position reported by the player.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressUpdate {
    pub media_id: u32,
    pub media_type: String,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    pub position: f64,
    pub duration: f64,
    /// set once the player decided this file is done, e.g. the credits were reached
    #[serde(default)]
    pub completed: bool,
    /// tmdb details (or a stored history item) to add the title to watch history with
    #[serde(default)]
    pub media: Option<Value>,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchProgressData {
    pub progress: HashMap<String, Value>,
}

pub struct WatchProgressManager {
    file_path: PathBuf,
    data: Arc<RwLock<WatchProgressData>>,
}

impl WatchProgressManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("watch_progress.json");
        let data = if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            WatchProgressData::default()
        };
        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    fn persist(file_path: &PathBuf, data: &WatchProgressData) {
        if let Ok(content) = serde_json::to_string_pretty(data) {
            let _ = fs::write(file_path, content);
        }
    }

    pub async fn get_all(&self) -> HashMap<String, Value> {
        self.data.read().await.progress.clone()
    }

    /// every entry with its `watched` flag filled in.
    pub async fn get_all_with_watched(&self) -> HashMap<String, Value> {
        self.data
            .read()
            .await
            .progress
            .iter()
            .map(|(k, v)| (k.clone(), with_watched_flag(v)))
            .collect()
    }

    pub async fn get_entry(&self, media_id: u32, media_type: &str) -> Option<ProgressEntry> {
        let data = self.data.read().await;
        data.progress
            .get(&progress_key(media_id, media_type))
            .and_then(ProgressEntry::from_value)
    }

    /// saves a player position under the title key and, for episodes, the episode
    /// key too. returns none when the player has no usable position yet.
    pub async fn record(&self, update: &ProgressUpdate) -> Option<ProgressEntry> {
        if update.media_id == 0 || !(update.position > 0.0) || !(update.duration > 0.0) {
            return None;
        }

        let episode = match (update.season, update.episode) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };
        let now = now_millis();
        let mut entry = ProgressEntry {
            current_timestamp: Some(update.position.floor()),
            duration: Some(update.duration.floor()),
            current_season: episode.map(|(s, _)| s),
            current_episode: episode.map(|(_, e)| e),
            completed: update.completed.then_some(true),
            updated_at: Some(now),
            completed_at: None,
        };

        let key = progress_key(update.media_id, &update.media_type);
        let ep_key = episode
            .filter(|(s, e)| *s > 0 && *e > 0)
            .map(|(s, e)| episode_key(update.media_id, &update.media_type, s, e));

        let mut data = self.data.write().await;

        // keep the first finish time so saves during the credits don't reorder history
        if entry.is_watched() {
            let previous = data
                .progress
                .get(ep_key.as_ref().unwrap_or(&key))
                .and_then(ProgressEntry::from_value)
                .filter(|p| p.is_watched())
                .and_then(|p| p.completed_at)
                .filter(|t| *t > 0);
            entry.completed_at = Some(previous.unwrap_or(now));
        }

        let value = serde_json::to_value(&entry).ok()?;
        data.progress.insert(key, value.clone());
        if let Some(ep_key) = ep_key {
            data.progress.insert(ep_key, value);
        }
        Self::persist(&self.file_path, &data);
        Some(entry)
    }

    pub async fn set_all(&self, progress: HashMap<String, Value>) {
        let mut data = self.data.write().await;
        data.progress = progress;
        Self::persist(&self.file_path, &data);
    }

    pub async fn update_entry(&self, key: String, value: Value) {
        let mut data = self.data.write().await;
        data.progress.insert(key, value);
        Self::persist(&self.file_path, &data);
    }

    pub async fn remove_entry(&self, key: String) {
        let mut data = self.data.write().await;
        data.progress.remove(&key);
        Self::persist(&self.file_path, &data);
    }

    pub async fn clear(&self) {
        let mut data = self.data.write().await;
        data.progress.clear();
        Self::persist(&self.file_path, &data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(position: f64, completed: bool) -> ProgressUpdate {
        ProgressUpdate {
            media_id: 42,
            media_type: "tv".to_string(),
            season: Some(1),
            episode: Some(3),
            position,
            duration: 1400.0,
            completed,
            media: None,
        }
    }

    #[tokio::test]
    async fn records_title_and_episode_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = WatchProgressManager::new(tmp.path().to_path_buf());

        assert!(manager.record(&update(0.0, false)).await.is_none());

        let entry = manager.record(&update(300.4, false)).await.unwrap();
        assert_eq!(entry.current_timestamp, Some(300.0));
        assert!(entry.completed_at.is_none());

        let all = manager.get_all_with_watched().await;
        assert_eq!(all["42-tv"]["currentEpisode"], 3);
        assert_eq!(all["42-tv-S1-E3"]["watched"], false);

        // the first finish time sticks while the credits keep saving
        let first = manager.record(&update(1300.0, true)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let later = manager.record(&update(1350.0, true)).await.unwrap();
        assert_eq!(first.completed_at, later.completed_at);
        assert_eq!(manager.get_all_with_watched().await["42-tv"]["watched"], true);

        // persisted to disk
        let reloaded = WatchProgressManager::new(tmp.path().to_path_buf());
        assert_eq!(reloaded.get_entry(42, "tv").await.unwrap().current_timestamp, Some(1350.0));
    }
}
