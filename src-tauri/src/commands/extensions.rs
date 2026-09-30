//! extension management and debrid streaming.

use magnolia_core::extensions::runtime::{DebridFile, DebridListContext, DebridResolveContext};
use magnolia_core::extensions::ExtensionInfo;
use magnolia_core::subtitles::Subtitle;
use std::collections::HashMap;

use super::CoreState;

#[tauri::command]
pub async fn list_extensions(core: CoreState<'_>) -> Result<Vec<ExtensionInfo>, String> {
    Ok(core.extensions.list().await)
}

#[tauri::command]
pub async fn install_extension_from_path(core: CoreState<'_>, path: String) -> Result<ExtensionInfo, String> {
    core.extensions.install_from_path(&path).await
}

#[tauri::command]
pub async fn install_extension_from_url(core: CoreState<'_>, url: String) -> Result<ExtensionInfo, String> {
    core.extensions.install_from_url(&url).await
}

#[tauri::command]
pub async fn remove_extension(core: CoreState<'_>, id: String) -> Result<(), String> {
    core.extensions.remove(&id).await
}

#[tauri::command]
pub async fn set_extension_enabled(core: CoreState<'_>, id: String, enabled: bool) -> Result<(), String> {
    core.extensions.set_enabled(&id, enabled).await
}

#[tauri::command]
pub async fn set_extension_field_values(
    core: CoreState<'_>,
    id: String,
    values: HashMap<String, String>,
) -> Result<(), String> {
    core.extensions.set_field_values(&id, values).await
}

#[tauri::command]
pub async fn fetch_extension_subtitles(
    core: CoreState<'_>,
    tmdb_id: String,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
    auto_only: bool,
) -> Result<Vec<Subtitle>, String> {
    Ok(core
        .extensions
        .fetch_subtitles(tmdb_id, media_type, season, episode, auto_only)
        .await)
}

#[tauri::command]
pub async fn list_debrid_files(
    core: CoreState<'_>,
    ext_id: String,
    magnet: String,
    season: Option<u32>,
    episode: Option<u32>,
    media_type: Option<String>,
) -> Result<Vec<DebridFile>, String> {
    let ctx = DebridListContext {
        magnet,
        season,
        episode,
        media_type,
    };
    core.extensions.list_debrid_files(&ext_id, ctx).await
}

#[tauri::command]
pub async fn resolve_debrid_stream(
    core: CoreState<'_>,
    ext_id: String,
    magnet: String,
    file_id: Option<i64>,
    file_name: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
    media_type: Option<String>,
) -> Result<String, String> {
    let ctx = DebridResolveContext {
        magnet,
        file_id,
        file_name,
        season,
        episode,
        media_type,
    };
    core.extensions.resolve_debrid_stream(&ext_id, ctx).await
}
