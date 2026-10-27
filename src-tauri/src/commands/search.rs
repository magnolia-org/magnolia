//! torrent search.

use magnolia_core::search::{magnet, parse_release, ReleaseInfo, SearchEvent, SearchResult, TorrentSearchRequest, TorrentSearchResponse};
use tauri::ipc::Channel;

use super::CoreState;

/// streams progress and results to `on_event` as trackers answer, then returns
/// every result, best first. starting a search cancels the one before it.
#[tauri::command]
pub async fn search_torrents(
    core: CoreState<'_>,
    request: TorrentSearchRequest,
    on_event: Channel<SearchEvent>,
) -> Result<TorrentSearchResponse, String> {
    Ok(core
        .search_torrents_streaming(request, |event| {
            if let Err(e) = on_event.send(event) {
                log::warn!("failed to send search event: {}", e);
            }
        })
        .await)
}

#[tauri::command]
pub fn cancel_torrent_search(core: CoreState<'_>) {
    core.cancel_torrent_search();
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

#[tauri::command]
pub fn parse_release_title(title: String) -> ReleaseInfo {
    parse_release(&title)
}
