use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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
