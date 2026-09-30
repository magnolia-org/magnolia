//! magnolia's backend: torrent streaming, tracker search, extensions and watch
//! tracking. nothing here depends on the ui layer; the desktop app wraps it in commands.

pub mod anime_list;
pub mod cache_metadata;
pub mod extensions;
pub mod files;
pub mod my_list;
pub mod playback;
pub mod search;
pub mod settings;
pub mod subtitle_packs;
pub mod subtitles;
pub mod torrent;
pub mod track_preferences;
pub mod tracking;
pub mod watch_history;
pub mod watch_progress;
pub mod watch_state;
pub mod watching;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anime_list::AnimeListManager;
use cache_metadata::CacheMetadataManager;
use extensions::ExtensionManager;
use my_list::MyListManager;
use settings::SettingsManager;
use subtitle_packs::SubtitlePackStore;
use torrent::TorrentManager;
use track_preferences::TrackPreferencesManager;
use tracking::TrackingManager;
use watch_history::WatchHistoryManager;
use watch_progress::WatchProgressManager;

/// every backend service, rooted at one data directory.
pub struct Core {
    data_dir: PathBuf,
    pub settings: SettingsManager,
    pub selections: TrackingManager,
    pub watch_history: WatchHistoryManager,
    pub watch_progress: WatchProgressManager,
    pub my_list: MyListManager,
    pub track_preferences: TrackPreferencesManager,
    pub anime_list: Arc<AnimeListManager>,
    pub cache_metadata: CacheMetadataManager,
    pub extensions: Arc<ExtensionManager>,
    pub subtitle_packs: SubtitlePackStore,
    pub torrents: Arc<TorrentManager>,
}

impl Core {
    /// loads persisted state from `data_dir` and starts the torrent session and its
    /// local streaming server. must run inside a tokio runtime.
    pub async fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;

        let torrents = TorrentManager::new(data_dir.join("torrents"), data_dir.join("fonts")).await?;

        Ok(Self {
            settings: SettingsManager::new(data_dir.clone()),
            selections: TrackingManager::new(data_dir.clone()),
            watch_history: WatchHistoryManager::new(data_dir.clone()),
            watch_progress: WatchProgressManager::new(data_dir.clone()),
            my_list: MyListManager::new(data_dir.clone()),
            track_preferences: TrackPreferencesManager::new(data_dir.clone()),
            anime_list: Arc::new(AnimeListManager::new(data_dir.clone())),
            cache_metadata: CacheMetadataManager::new(data_dir.clone()),
            extensions: Arc::new(ExtensionManager::new(data_dir.clone())),
            subtitle_packs: SubtitlePackStore::new(data_dir.join("subtitle_packs")),
            torrents: Arc::new(torrents),
            data_dir,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// the anime list only covers tv ids, so anything else falls back to
    /// `search::looks_like_anime` on its genres and original language.
    pub async fn is_anime(
        &self,
        tmdb_id: u32,
        media_type: Option<&str>,
        genre_ids: &[u32],
        original_language: Option<&str>,
    ) -> bool {
        if media_type != Some("movie") && self.anime_list.is_anime(tmdb_id).await {
            return true;
        }
        search::looks_like_anime(genre_ids, original_language)
    }
}
