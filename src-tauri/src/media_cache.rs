use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub enum TrackType {
    Subtitle,
    Audio,
    Torrent,
}

impl TrackType {
    fn folder_name(&self) -> &str {
        match self {
            TrackType::Subtitle => "subtitles",
            TrackType::Audio => "audio",
            TrackType::Torrent => "torrents",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CacheGroup {
    pub id: String,
    pub total_size: u64,
    pub audio_size: u64,
    pub subtitle_size: u64,
    pub torrent_size: u64,
    pub audio_files: usize,
    pub subtitle_files: usize,
    pub torrent_files: usize,
}

pub struct MediaCache {
    base_dir: PathBuf,
    _lock: Arc<RwLock<()>>,
}

impl MediaCache {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            base_dir: app_data_dir,
            _lock: Arc::new(RwLock::new(())),
        }
    }

    fn get_cache_dir(&self, track_type: TrackType) -> PathBuf {
        let cache_dir = self.base_dir.join(track_type.folder_name());
        if !cache_dir.exists() {
            let _ = fs::create_dir_all(&cache_dir);
        }
        cache_dir
    }

    fn get_cache_path(&self, track_type: TrackType, cache_id: &str, file_index: usize, track_index: usize) -> PathBuf {
        let filename = format!("{}_{}_{}_{}.cache", 
            cache_id, 
            file_index, 
            track_index,
            self.hash_key(cache_id, file_index, track_index)
        );
        self.get_cache_dir(track_type).join(filename)
    }

    fn hash_key(&self, cache_id: &str, file_index: usize, track_index: usize) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}-{}-{}", cache_id, file_index, track_index));
        let result = hasher.finalize();
        format!("{:x}", result)[..8].to_string()
    }

    // Helper to recursively calculate directory size
    fn get_dir_size(path: &PathBuf) -> u64 {
        let mut size = 0;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        size += Self::get_dir_size(&entry.path());
                    } else {
                        size += metadata.len();
                    }
                }
            }
        }
        size
    }

    pub async fn get_cache_stats(&self) -> Result<Vec<CacheGroup>, String> {
        let mut groups: HashMap<String, CacheGroup> = HashMap::new();
        
        // Process Audio and Subtitle tracks
        for track_type in [TrackType::Audio, TrackType::Subtitle] {
            let cache_dir = self.get_cache_dir(track_type);
            if let Ok(entries) = fs::read_dir(&cache_dir) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_file() {
                            if let Some(filename) = entry.file_name().to_str() {
                                // Filename format: {cache_id}_{file_index}_{track_index}_{hash}.cache
                                let parts: Vec<&str> = filename.split('_').collect();
                                if parts.len() >= 4 {
                                    let cache_id = parts[0].to_string();
                                    let size = metadata.len();
                                    
                                    let group = groups.entry(cache_id.clone()).or_insert(CacheGroup {
                                        id: cache_id,
                                        total_size: 0,
                                        audio_size: 0,
                                        subtitle_size: 0,
                                        torrent_size: 0,
                                        audio_files: 0,
                                        subtitle_files: 0,
                                        torrent_files: 0,
                                    });
                                    
                                    group.total_size += size;
                                    match track_type {
                                        TrackType::Audio => {
                                            group.audio_size += size;
                                            group.audio_files += 1;
                                        },
                                        TrackType::Subtitle => {
                                            group.subtitle_size += size;
                                            group.subtitle_files += 1;
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Process Torrents
        // Torrents are stored in folders, but we don't have a direct mapping to cache_id (TMDB ID)
        // unless we can infer it. For now, we'll just list them as "Unknown" or try to match if possible.
        // However, the user wants them counted.
        // Since we can't easily map torrent folders to TMDB IDs without a database lookup or naming convention,
        // we will aggregate them into a special group "Torrents" or try to be smart.
        // BUT, the user asked to "make sure stuff in the torrents folder is also counted as cache".
        // If we can't map to ID, maybe we just show them as a separate item or under "Unknown".
        // Let's just add them to a group called "Torrents" for now if we can't map them.
        // OR, if the torrent folder name contains the ID? No, usually it's the torrent name.
        
        let torrents_dir = self.get_cache_dir(TrackType::Torrent);
        if let Ok(entries) = fs::read_dir(&torrents_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    let size = if metadata.is_dir() {
                        Self::get_dir_size(&entry.path())
                    } else {
                        metadata.len()
                    };
                    
                    // For torrents, we might not have the ID. 
                    // We'll use the folder/file name as the ID for display purposes if we can't map it.
                    // But the UI expects TMDB IDs to fetch metadata.
                    // If we use the name, the UI will show "ID: {name}".
                    let name = entry.file_name().to_string_lossy().to_string();
                    
                    // Check if we can match this to an existing group (unlikely without more info)
                    // So we create a new group for each torrent folder/file
                    // We prefix with "torrent_" to avoid collision with TMDB IDs if they happen to be numbers (unlikely for names)
                    let group_id = format!("torrent_{}", name);
                    
                    let group = groups.entry(group_id.clone()).or_insert(CacheGroup {
                        id: group_id, // This will be treated as the ID
                        total_size: 0,
                        audio_size: 0,
                        subtitle_size: 0,
                        torrent_size: 0,
                        audio_files: 0,
                        subtitle_files: 0,
                        torrent_files: 0,
                    });
                    
                    group.total_size += size;
                    group.torrent_size += size;
                    group.torrent_files += 1; // Count the folder as 1 "file" or item
                }
            }
        }
        
        Ok(groups.into_values().collect())
    }

    pub async fn clear_cache_by_id(&self, target_id: &str) -> Result<(), String> {
        // Handle torrent deletion (IDs prefixed with "torrent_")
        if target_id.starts_with("torrent_") {
            let torrents_dir = self.get_cache_dir(TrackType::Torrent);
            let folder_name = target_id.strip_prefix("torrent_").unwrap_or(target_id);
            let torrent_path = torrents_dir.join(folder_name);
            
            if torrent_path.exists() {
                if torrent_path.is_dir() {
                    fs::remove_dir_all(&torrent_path)
                        .map_err(|e| format!("Failed to remove torrent directory: {}", e))?;
                } else {
                    fs::remove_file(&torrent_path)
                        .map_err(|e| format!("Failed to remove torrent file: {}", e))?;
                }
            }
            return Ok(());
        }
        
        // Handle regular cache deletion (audio/subtitle)
        for track_type in [TrackType::Audio, TrackType::Subtitle] {
            let cache_dir = self.get_cache_dir(track_type);
            if let Ok(entries) = fs::read_dir(&cache_dir) {
                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        if filename.starts_with(&format!("{}_", target_id)) {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn save_track(&self, track_type: TrackType, cache_id: &str, file_index: usize, track_index: usize, data: Vec<u8>) -> Result<(), String> {
        let path = self.get_cache_path(track_type, cache_id, file_index, track_index);
        fs::write(&path, data).map_err(|e| format!("Failed to save track cache: {}", e))?;
        println!("[{:?} Cache] Saved to {:?}", match track_type {
            TrackType::Subtitle => "Subtitle",
            TrackType::Audio => "Audio",
            TrackType::Torrent => "Torrent",
        }, path);
        Ok(())
    }

    pub async fn load_track(&self, track_type: TrackType, cache_id: &str, file_index: usize, track_index: usize) -> Result<Option<Vec<u8>>, String> {
        let path = self.get_cache_path(track_type, cache_id, file_index, track_index);
        if path.exists() {
            let data = fs::read(&path).map_err(|e| format!("Failed to load track cache: {}", e))?;
            println!("[{:?} Cache] Loaded {} bytes from {:?}", match track_type {
                TrackType::Subtitle => "Subtitle",
                TrackType::Audio => "Audio",
                TrackType::Torrent => "Torrent",
            }, data.len(), path);
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    pub async fn clear_cache(&self, track_type: TrackType) -> Result<(), String> {
        let cache_dir = self.get_cache_dir(track_type);
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir)
                .map_err(|e| format!("Failed to clear cache: {}", e))?;
            fs::create_dir_all(&cache_dir)
                .map_err(|e| format!("Failed to recreate cache dir: {}", e))?;
            println!("[{:?} Cache] Cleared all cached tracks", match track_type {
                TrackType::Subtitle => "Subtitle",
                TrackType::Audio => "Audio",
                TrackType::Torrent => "Torrent",
            });
        }
        Ok(())
    }
}
