//! watch tracking, my list, settings and other saved state.

use magnolia_core::cache_metadata::CacheMetadata;
use magnolia_core::settings::Settings;
use magnolia_core::track_preferences::TrackPreference;
use magnolia_core::watch_progress::ProgressUpdate;
use magnolia_core::watch_state::{ProgressEntry, ResumeTarget, SeasonInfo, WatchHistoryEntry};
use serde_json::Value;
use std::collections::HashMap;
use tauri::AppHandle;

use super::{notify_watch_state_changed, CoreState};

#[tauri::command]
pub async fn record_watch_progress(
    app: AppHandle,
    core: CoreState<'_>,
    update: ProgressUpdate,
) -> Result<Option<ProgressEntry>, String> {
    let entry = core.record_watch_progress(update).await;
    if entry.is_some() {
        notify_watch_state_changed(&app);
    }
    Ok(entry)
}

#[tauri::command]
pub async fn get_watch_history(core: CoreState<'_>) -> Result<Vec<WatchHistoryEntry>, String> {
    Ok(core.watch_history_sorted().await)
}

#[tauri::command]
pub async fn remove_watch_history_item(
    app: AppHandle,
    core: CoreState<'_>,
    media_id: u32,
    media_type: String,
) -> Result<(), String> {
    core.watch_history.remove_item(media_id, media_type).await;
    notify_watch_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn clear_watch_history(app: AppHandle, core: CoreState<'_>) -> Result<(), String> {
    core.watch_history.clear().await;
    notify_watch_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_watch_progress(core: CoreState<'_>) -> Result<HashMap<String, Value>, String> {
    Ok(core.watch_progress.get_all_with_watched().await)
}

#[tauri::command]
pub async fn set_watch_progress(
    app: AppHandle,
    core: CoreState<'_>,
    progress: HashMap<String, Value>,
) -> Result<(), String> {
    core.watch_progress.set_all(progress).await;
    notify_watch_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn remove_watch_progress_entry(app: AppHandle, core: CoreState<'_>, key: String) -> Result<(), String> {
    core.watch_progress.remove_entry(key).await;
    notify_watch_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn clear_watch_progress(app: AppHandle, core: CoreState<'_>) -> Result<(), String> {
    core.watch_progress.clear().await;
    notify_watch_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_resume_target(
    core: CoreState<'_>,
    media_id: u32,
    media_type: String,
    seasons: Option<Vec<SeasonInfo>>,
    progress: Option<Value>,
) -> Result<Option<ResumeTarget>, String> {
    Ok(core
        .resume_target(media_id, &media_type, &seasons.unwrap_or_default(), progress)
        .await)
}

#[tauri::command]
pub async fn get_my_list(core: CoreState<'_>) -> Result<Vec<Value>, String> {
    Ok(core.my_list.get_list().await)
}

#[tauri::command]
pub async fn set_my_list(core: CoreState<'_>, items: Vec<Value>) -> Result<(), String> {
    core.my_list.set_list(items).await;
    Ok(())
}

#[tauri::command]
pub async fn toggle_my_list_item(core: CoreState<'_>, item: Value) -> Result<Vec<Value>, String> {
    Ok(core.my_list.toggle_item(item).await)
}

#[tauri::command]
pub async fn save_track_preference(
    core: CoreState<'_>,
    magnet_link: String,
    audio_track_id: Option<i64>,
    subtitle_track_id: Option<i64>,
    subtitle_language: Option<String>,
    subtitle_offset: Option<f64>,
) -> Result<(), String> {
    core.track_preferences
        .save_preference(magnet_link, audio_track_id, subtitle_track_id, subtitle_language, subtitle_offset)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn get_track_preference(core: CoreState<'_>, magnet_link: String) -> Result<Option<TrackPreference>, String> {
    Ok(core.track_preferences.get_preference(&magnet_link).await)
}

#[tauri::command]
pub async fn save_settings(core: CoreState<'_>, settings: Settings) -> Result<(), String> {
    core.settings.save(settings).await;
    Ok(())
}

#[tauri::command]
pub async fn get_settings(core: CoreState<'_>) -> Result<Settings, String> {
    Ok(core.settings.get().await)
}

#[tauri::command]
pub async fn check_is_anime(
    core: CoreState<'_>,
    tmdb_id: u32,
    media_type: Option<String>,
    genre_ids: Option<Vec<u32>>,
) -> Result<bool, String> {
    Ok(core
        .is_anime(tmdb_id, media_type.as_deref(), &genre_ids.unwrap_or_default())
        .await)
}

#[tauri::command]
pub async fn refresh_anime_list(core: CoreState<'_>) -> Result<(), String> {
    core.anime_list.refresh().await
}

#[tauri::command]
pub fn save_cache_metadata(core: CoreState<'_>, hash: String, tmdb_id: u32, media_type: String) -> Result<(), String> {
    core.cache_metadata.set_mapping(hash, tmdb_id, media_type)
}

#[tauri::command]
pub fn get_cache_metadata(core: CoreState<'_>, hash: String) -> Result<Option<CacheMetadata>, String> {
    Ok(core.cache_metadata.get_mapping(&hash))
}

#[tauri::command]
pub fn get_all_cache_metadata(core: CoreState<'_>) -> Result<HashMap<String, CacheMetadata>, String> {
    Ok(core.cache_metadata.all())
}
