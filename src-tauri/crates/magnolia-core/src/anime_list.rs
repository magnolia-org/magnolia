use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    map_file: PathBuf,
    tv_ids: Arc<RwLock<HashSet<u32>>>,
    /// tmdb tv id to the anilist ids of its tv and ona entries, one per season or cour
    anilist_ids: Arc<RwLock<HashMap<u32, Vec<u32>>>>,
}

impl AnimeListManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let ids_file = app_data_dir.join("anime_tv_ids.json");
        let meta_file = app_data_dir.join("anime_tv_ids_meta.json");
        let map_file = app_data_dir.join("anime_tv_anilist.json");

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

        let anilist_ids: HashMap<u32, Vec<u32>> = fs::read_to_string(&map_file)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();

        Self {
            ids_file,
            meta_file,
            map_file,
            tv_ids: Arc::new(RwLock::new(tv_ids)),
            anilist_ids: Arc::new(RwLock::new(anilist_ids)),
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
        // lists saved before the anilist map existed need one fresh download
        if !self.map_file.exists() {
            return true;
        }
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
        let mut anilist_ids: HashMap<u32, Vec<u32>> = HashMap::new();
        for entry in &entries {
            if let Some(tmdb) = entry.get("themoviedb_id") {
                if let Some(tv_id) = tmdb.get("tv").and_then(|v| v.as_u64()) {
                    ids.insert(tv_id as u32);
                    let is_series = matches!(entry.get("type").and_then(|t| t.as_str()), Some("TV" | "ONA"));
                    if let Some(anilist) = entry.get("anilist_id").and_then(|v| v.as_u64()).filter(|_| is_series) {
                        anilist_ids.entry(tv_id as u32).or_default().push(anilist as u32);
                    }
                }
            }
        }
        let map_json = serde_json::to_string(&anilist_ids).map_err(|e| e.to_string())?;
        fs::write(&self.map_file, map_json).map_err(|e| e.to_string())?;
        *self.anilist_ids.write().await = anilist_ids;

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

    /// the anilist entries (seasons, cours) of a tmdb show, in list order.
    pub async fn anilist_ids(&self, tmdb_id: u32) -> Vec<u32> {
        self.anilist_ids.read().await.get(&tmdb_id).cloned().unwrap_or_default()
    }

    pub async fn is_anime(&self, tmdb_id: u32) -> bool {
        let guard = self.tv_ids.read().await;
        guard.contains(&tmdb_id)
    }
}
