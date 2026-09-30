//! torrents, saved torrent selections and playback planning.

use magnolia_core::extensions::ExtensionInfo;
use magnolia_core::files::MediaFile;
use magnolia_core::playback::{PlaybackPlan, PlaybackRequest, TorrentFiles};
use magnolia_core::torrent::{StreamStatus, TorrentInfo};
use magnolia_core::tracking::{EpisodeTorrent, ShowHistory};

use super::CoreState;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn add_torrent(core: CoreState<'_>, magnet_or_url: String) -> Result<usize, String> {
    core.torrents.add_torrent(magnet_or_url).await.map_err(err)
}

#[tauri::command]
pub async fn get_torrent_info(core: CoreState<'_>, handle_id: usize) -> Result<TorrentInfo, String> {
    core.torrents.get_torrent_info(handle_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_torrents(core: CoreState<'_>) -> Result<Vec<TorrentInfo>, String> {
    core.torrents.list_torrents().await.map_err(err)
}

#[tauri::command]
pub async fn prepare_stream(core: CoreState<'_>, handle_id: usize, file_index: usize) -> Result<(), String> {
    core.torrents.prepare_stream(handle_id, file_index).await.map_err(err)
}

#[tauri::command]
pub async fn get_stream_status(core: CoreState<'_>, handle_id: usize, file_index: usize) -> Result<StreamStatus, String> {
    core.torrents.get_stream_status(handle_id, file_index).await.map_err(err)
}

#[tauri::command]
pub async fn pause_torrent(core: CoreState<'_>, handle_id: usize) -> Result<(), String> {
    core.torrents.pause_torrent(handle_id).await.map_err(err)
}

#[tauri::command]
pub async fn resume_torrent(core: CoreState<'_>, handle_id: usize) -> Result<(), String> {
    core.torrents.resume_torrent(handle_id).await.map_err(err)
}

#[tauri::command]
pub async fn remove_torrent(core: CoreState<'_>, handle_id: usize, delete_files: bool) -> Result<(), String> {
    core.torrents.remove_torrent(handle_id, delete_files).await.map_err(err)
}

#[tauri::command]
pub async fn stop_stream(core: CoreState<'_>, handle_id: usize, delete_files: bool) -> Result<(), String> {
    core.torrents.stop_stream(handle_id, delete_files).await.map_err(err)
}

#[tauri::command]
pub async fn wipe_all_torrent_files(core: CoreState<'_>) -> Result<(), String> {
    core.torrents.wipe_all_files().await.map_err(err)
}

#[tauri::command]
pub async fn get_torrent_piece_ranges(
    core: CoreState<'_>,
    handle_id: usize,
    file_index: usize,
) -> Result<Vec<(f64, f64)>, String> {
    core.torrents.get_piece_ranges(handle_id, file_index).await.map_err(err)
}

#[tauri::command]
pub async fn get_download_dir(core: CoreState<'_>) -> Result<String, String> {
    Ok(core.torrents.get_download_dir().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_http_port(core: CoreState<'_>) -> Result<u16, String> {
    core.torrents.get_http_port().await
}

#[tauri::command]
pub async fn save_torrent_selection(
    core: CoreState<'_>,
    show_id: u32,
    season: u32,
    episode: u32,
    magnet_link: String,
    file_index: usize,
) -> Result<(), String> {
    core.selections
        .save_selection(show_id, season, episode, magnet_link, file_index)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn save_multiple_torrent_selections(
    core: CoreState<'_>,
    show_id: u32,
    selections: Vec<(u32, u32, String, usize)>,
) -> Result<(), String> {
    core.selections.save_multiple_selections(show_id, selections).await;
    Ok(())
}

#[tauri::command]
pub async fn get_saved_selection(
    core: CoreState<'_>,
    show_id: u32,
    season: u32,
    episode: u32,
) -> Result<Option<EpisodeTorrent>, String> {
    Ok(core.selections.get_selection(show_id, season, episode).await)
}

#[tauri::command]
pub async fn get_all_torrent_selections(core: CoreState<'_>, show_id: u32) -> Result<Option<ShowHistory>, String> {
    Ok(core.selections.get_all_selections(show_id).await)
}

#[tauri::command]
pub async fn remove_saved_selection(core: CoreState<'_>, show_id: u32, season: u32, episode: u32) -> Result<(), String> {
    core.selections.remove_selection(show_id, season, episode).await;
    Ok(())
}

#[tauri::command]
pub async fn remove_torrent_all_assignments(
    core: CoreState<'_>,
    show_id: u32,
    magnet_link: String,
) -> Result<(), String> {
    core.selections.remove_all_by_magnet(show_id, magnet_link).await;
    Ok(())
}

#[tauri::command]
pub async fn get_streaming_client(core: CoreState<'_>) -> Result<Option<ExtensionInfo>, String> {
    Ok(core.streaming_client().await.as_ref().map(Into::into))
}

#[tauri::command]
pub async fn get_torrent_files(
    core: CoreState<'_>,
    magnet_link: String,
    season: Option<u32>,
    episode: Option<u32>,
    media_type: Option<String>,
) -> Result<TorrentFiles, String> {
    core.torrent_files(&magnet_link, season, episode, media_type).await
}

#[tauri::command]
pub async fn auto_assign_torrent_files(
    core: CoreState<'_>,
    show_id: u32,
    media_type: String,
    season: u32,
    episode: u32,
    magnet_link: String,
    files: Vec<MediaFile>,
) -> Result<Option<MediaFile>, String> {
    Ok(core
        .auto_assign_files(show_id, &media_type, season, episode, &magnet_link, &files)
        .await)
}

#[tauri::command]
pub async fn prepare_playback(core: CoreState<'_>, request: PlaybackRequest) -> Result<PlaybackPlan, String> {
    core.prepare_playback(request).await
}

#[tauri::command]
pub async fn prepare_saved_playback(
    core: CoreState<'_>,
    media_id: u32,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Option<PlaybackPlan>, String> {
    core.prepare_saved_playback(media_id, &media_type, season, episode).await
}

#[tauri::command]
pub async fn plan_quick_play(core: CoreState<'_>, media_id: u32, media_type: String) -> Result<Option<PlaybackPlan>, String> {
    core.plan_quick_play(media_id, &media_type).await
}
