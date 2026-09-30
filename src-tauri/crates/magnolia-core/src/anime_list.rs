use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Serialize, Deserialize, Default)]
struct AnimeListMeta {
    last_fetched: i64,
}

pub struct AnimeListManager {
    ids_file: PathBuf,
    meta_file: PathBuf,
    tv_ids: Arc<RwLock<HashSet<u32>>>,
}

impl AnimeListManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let ids_file = app_data_dir.join("anime_tv_ids.json");
        let meta_file = app_data_dir.join("anime_tv_ids_meta.json");

        let tv_ids = if ids_file.exists() {
            match fs::read_to_string(&ids_file) {
                Ok(content) => {
                    let ids: Vec<u32> = serde_json::from_str(&content).unwrap_or_default();
                    ids.into_iter().collect()
                }
                Err(_) => HashSet::new(),
            }
        } else {
            HashSet::new()
        };

        Self {
            ids_file,
            meta_file,
            tv_ids: Arc::new(RwLock::new(tv_ids)),
        }
    }

    fn last_fetched_secs(&self) -> i64 {
        if !self.meta_file.exists() {
            return 0;
        }
        let content = fs::read_to_string(&self.meta_file).unwrap_or_default();
        let meta: AnimeListMeta = serde_json::from_str(&content).unwrap_or_default();
        meta.last_fetched
    }

    pub fn is_stale(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now - self.last_fetched_secs() > 24 * 60 * 60
    }

    pub async fn ensure_fresh(&self) {
        if self.is_stale() {
            if let Err(e) = self.refresh().await {
                eprintln!("Failed to refresh anime list: {}", e);
            }
        }
    }

    pub async fn refresh(&self) -> Result<(), String> {
        const URL: &str = "https://raw.githubusercontent.com/Fribb/anime-lists/refs/heads/master/anime-list-full.json";

        let text = reqwest::get(URL)
            .await
            .map_err(|e| format!("failed to fetch anime list: {}", e))?
            .text()
            .await
            .map_err(|e| format!("failed to read anime list response: {}", e))?;

        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&text).map_err(|e| format!("failed to parse anime list: {}", e))?;

        let mut ids: HashSet<u32> = HashSet::new();
        for entry in &entries {
            if let Some(tmdb) = entry.get("themoviedb_id") {
                if let Some(tv_id) = tmdb.get("tv").and_then(|v| v.as_u64()) {
                    ids.insert(tv_id as u32);
                }
            }
        }

        let ids_vec: Vec<u32> = ids.iter().cloned().collect();
        let ids_json = serde_json::to_string(&ids_vec).map_err(|e| e.to_string())?;
        fs::write(&self.ids_file, ids_json).map_err(|e| e.to_string())?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let meta = AnimeListMeta { last_fetched: now };
        let meta_json = serde_json::to_string(&meta).map_err(|e| e.to_string())?;
        fs::write(&self.meta_file, meta_json).map_err(|e| e.to_string())?;

        let count = ids.len();
        let mut guard = self.tv_ids.write().await;
        *guard = ids;
        println!("Anime list refreshed: {} TV IDs cached", count);
        Ok(())
    }

    pub async fn is_anime(&self, tmdb_id: u32) -> bool {
        let guard = self.tv_ids.read().await;
        guard.contains(&tmdb_id)
    }
}
