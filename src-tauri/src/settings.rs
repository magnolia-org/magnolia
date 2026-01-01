use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub external_player: String,
    pub remember_preferences: bool,
    pub show_skip_prompts: bool,
    #[serde(default)]
    pub hide_recommendations: bool,
    #[serde(default)]
    pub clear_cache_after_watch: bool,
    #[serde(default = "default_true")]
    pub check_for_updates: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            external_player: "vlc".to_string(),
            remember_preferences: true,
            show_skip_prompts: true,
            hide_recommendations: false,
            clear_cache_after_watch: false,
            check_for_updates: true,
        }
    }
}

pub struct SettingsManager {
    file_path: PathBuf,
    data: Arc<RwLock<Settings>>,
}

impl SettingsManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file_path = app_data_dir.join("settings.json");
        
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        
        let data = if file_path.exists() {
            match fs::read_to_string(&file_path) {
                Ok(content) => {
                    match serde_json::from_str(&content) {
                        Ok(settings) => {
                            println!("loaded settings from {:?}", file_path);
                            settings
                        }
                        Err(e) => {
                            eprintln!("failed to parse settings file: {}, using defaults", e);
                            Settings::default()
                        }
                    }
                }
                Err(e) => {
                    eprintln!("failed to read settings file: {}, using defaults", e);
                    Settings::default()
                }
            }
        } else {
            println!("no settings file found, using defaults");
            Settings::default()
        };

        Self {
            file_path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub async fn save(&self, settings: Settings) {
        let mut data = self.data.write().await;
        *data = settings.clone();

        match serde_json::to_string_pretty(&settings) {
            Ok(content) => {
                match fs::write(&self.file_path, content) {
                    Ok(_) => println!("settings saved to {:?}", self.file_path),
                    Err(e) => eprintln!("failed to write settings file: {}", e),
                }
            }
            Err(e) => eprintln!("failed to serialize settings: {}", e),
        }
    }

    pub async fn get(&self) -> Settings {
        let data = self.data.read().await;
        data.clone()
    }
}
