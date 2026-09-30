//! online subtitles and imported subtitle packs.

use magnolia_core::subtitle_packs::ImportResult;
use magnolia_core::subtitles::{self, Subtitle};
use std::collections::HashMap;

use super::CoreState;

#[tauri::command]
pub async fn fetch_subtitles(
    tmdb_id: String,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Subtitle>, String> {
    subtitles::fetch_subtitles(tmdb_id, media_type, season, episode).await
}

#[tauri::command]
pub async fn download_subtitle(url: String) -> Result<String, String> {
    subtitles::download_subtitle(url).await
}

#[tauri::command]
pub async fn import_subtitle_pack(core: CoreState<'_>, show_id: i64, paths: Vec<String>) -> Result<ImportResult, String> {
    let core = core.inner().clone();
    // walking folders and unpacking zips is blocking file io
    tokio::task::spawn_blocking(move || core.subtitle_packs.import(show_id, &paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_subtitle_pack_for_episode(
    core: CoreState<'_>,
    show_id: i64,
    season: u32,
    episode: u32,
) -> Result<Option<String>, String> {
    Ok(core.subtitle_packs.episode_file(show_id, season, episode))
}

#[tauri::command]
pub async fn get_subtitle_pack_coverage(core: CoreState<'_>, show_id: i64) -> Result<HashMap<String, Vec<u32>>, String> {
    Ok(core.subtitle_packs.coverage(show_id))
}

#[tauri::command]
pub async fn remove_subtitle_pack_episode(
    core: CoreState<'_>,
    show_id: i64,
    season: u32,
    episode: u32,
) -> Result<(), String> {
    core.subtitle_packs.remove_episode(show_id, season, episode)
}

#[tauri::command]
pub async fn clear_subtitle_pack(core: CoreState<'_>, show_id: i64) -> Result<(), String> {
    core.subtitle_packs.clear(show_id)
}
