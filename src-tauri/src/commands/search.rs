//! torrent search.

use magnolia_core::search::{
    magnet, parse_audio_codec, parse_title_metadata, SearchResult, TitleMeta, TorrentSearchRequest,
    TorrentSearchResponse,
};
use serde::Serialize;

use super::CoreState;

#[tauri::command]
pub async fn search_torrents(core: CoreState<'_>, request: TorrentSearchRequest) -> Result<TorrentSearchResponse, String> {
    Ok(core.search_torrents(request).await)
}

#[tauri::command]
pub async fn get_tracker_preference(core: CoreState<'_>) -> Result<Vec<String>, String> {
    Ok(core.settings.get().await.tracker_preference)
}

#[tauri::command]
pub async fn set_tracker_preference(core: CoreState<'_>, trackers: Vec<String>) -> Result<(), String> {
    core.settings.set_tracker_preference(trackers).await;
    Ok(())
}

/// a search result for a magnet link the user pasted.
#[tauri::command]
pub fn parse_magnet_link(link: String) -> Result<SearchResult, String> {
    magnet::result_from_magnet(&link)
}

#[derive(Serialize)]
pub struct ReleaseTitleInfo {
    #[serde(flatten)]
    meta: TitleMeta,
    audio_codec: Option<String>,
}

#[tauri::command]
pub fn parse_release_title(title: String) -> ReleaseTitleInfo {
    ReleaseTitleInfo {
        meta: parse_title_metadata(&title),
        audio_codec: parse_audio_codec(&title),
    }
}
