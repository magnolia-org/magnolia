use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MyListData {
    pub items: Vec<Value>,
}

pub struct MyListManager {
    file_path: PathBuf,
    data: Arc<RwLock<MyListData>>,
}

impl MyListManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("my_list.json");
        let data = if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            MyListData::default()
        };
        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    fn persist(file_path: &PathBuf, data: &MyListData) {
        if let Ok(content) = serde_json::to_string_pretty(data) {
            let _ = fs::write(file_path, content);
        }
    }

    pub async fn get_list(&self) -> Vec<Value> {
        self.data.read().await.items.clone()
    }

    pub async fn set_list(&self, items: Vec<Value>) {
        let mut data = self.data.write().await;
        data.items = items;
        Self::persist(&self.file_path, &data);
    }

    pub async fn toggle_item(&self, item: Value) -> Vec<Value> {
        let mut data = self.data.write().await;
        let id = item.get("id").and_then(|v| v.as_u64());
        let media_type = item
            .get("media_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let (Some(id), Some(mt)) = (id, media_type) {
            let pos = data.items.iter().position(|existing| {
                existing.get("id").and_then(|v| v.as_u64()) == Some(id)
                    && existing.get("media_type").and_then(|v| v.as_str()) == Some(mt.as_str())
            });
            if let Some(i) = pos {
                data.items.remove(i);
            } else {
                data.items.insert(0, item);
            }
        }

        Self::persist(&self.file_path, &data);
        data.items.clone()
    }
}
