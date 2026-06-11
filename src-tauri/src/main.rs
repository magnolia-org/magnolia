
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mpv;
mod search;
mod torrent;
mod tracking;
mod watch_history;
mod watch_progress;
mod my_list;
mod track_preferences;
mod settings;
mod logger;
mod cache_metadata;
mod subtitles;
mod subtitle_packs;
mod anime_list;
mod updater;
mod extensions;

use extensions::ExtensionManager;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Manager, State};
use torrent::TorrentManager;
use tracking::TrackingManager;
use watch_history::{WatchHistoryManager, WatchHistoryItem};
use watch_progress::WatchProgressManager;
use my_list::MyListManager;
use track_preferences::TrackPreferencesManager;
use settings::{SettingsManager, Settings};
use anime_list::AnimeListManager;
use logger::Logger;
use cache_metadata::CacheMetadataManager;

/// Tauri managed state — accessible by mpv event_loop and all mpv commands.
pub struct AppState {
    pub mpv_player: Arc<Mutex<mpv::MpvHandle>>,
}


// ── mpv commands ────────────────────────────────────────────────────────────

#[tauri::command]
async fn load_file(
    state: State<'_, AppState>,
    path: String,
    #[allow(non_snake_case)] resumePosition: Option<f64>,
    #[allow(non_snake_case)] autoPlay: Option<bool>,
) -> Result<(), String> {
    log::info!("[player-init] load_file: issuing loadfile to mpv, url={}", &path);
    let t0 = std::time::Instant::now();
    let mpv = state.mpv_player.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let lock_t = std::time::Instant::now();
        let guard = mpv.lock().map_err(|e| e.to_string())?;
        log::info!("[player-init] load_file: mpv lock acquired in {:.3}s", lock_t.elapsed().as_secs_f64());
        let start_opt;
        let mut args: Vec<&str> = vec!["loadfile", &path, "replace"];
        if let Some(pos) = resumePosition {
            if pos > 0.0 {
                start_opt = format!("start={pos}");
                args.push(&start_opt);
            }
        }
        guard.command(&args);
        log::info!("[player-init] load_file: loadfile command sent in {:.3}s total", t0.elapsed().as_secs_f64());
        let pause_val = if autoPlay.unwrap_or(true) { "no" } else { "yes" };
        guard.set_option_string("pause", pause_val);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cycle_pause(state: State<'_, AppState>) -> Result<(), String> {
    let mpv = state.mpv_player.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let guard = mpv.lock().map_err(|e| e.to_string())?;
        if guard.eof_reached() {
            guard.command(&["seek", "0", "absolute"]);
            guard.set_option_string("pause", "no");
        } else {
            guard.command(&["cycle", "pause"]);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn seek_video(state: State<'_, AppState>, seconds: f64) -> Result<(), String> {
    let mpv = state.mpv_player.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let guard = mpv.lock().map_err(|e| e.to_string())?;
        let pos_str = seconds.to_string();
        guard.command(&["seek", &pos_str, "absolute"]);
        if guard.eof_reached() {
            guard.set_option_string("pause", "no");
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn mpv_run_command(
    state: State<'_, AppState>,
    args: Vec<String>,
) -> Result<(), String> {
    let mpv = state.mpv_player.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let guard = mpv.lock().map_err(|e| e.to_string())?;
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        guard.command(&refs);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn mpv_set_option_string(
    state: State<'_, AppState>,
    name: String,
    value: String,
) -> Result<(), String> {
    let mpv = state.mpv_player.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let guard = mpv.lock().map_err(|e| e.to_string())?;
        guard.set_option_string(&name, &value);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── search commands ──────────────────────────────────────────────────────────

/// Run a set of tracker extensions in parallel and collect their results.
async fn search_tracker_extensions(
    trackers: Vec<extensions::LoadedExtension>,
    query: String,
    media_type: Option<String>,
    imdb_id: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Vec<search::SearchResult> {
    let mut handles = vec![];

    for ext in trackers {
        let ctx = extensions::runtime::TrackerSearchContext {
            query: query.clone(),
            media_type: media_type.clone(),
            imdb_id: imdb_id.clone(),
            season,
            episode,
        };
        // Rhai execution and its HTTP calls are blocking
        handles.push(tokio::task::spawn_blocking(move || {
            let result = extensions::runtime::run_tracker_search(
                &ext.source,
                &ext.manifest,
                &ext.field_values,
                &ctx,
            );
            (ext.id, result)
        }));
    }

    let mut all_results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((id, Ok(results))) => {
                println!("tracker extension {} returned {} results", id, results.len());
                all_results.extend(results);
            }
            Ok((id, Err(e))) => println!("tracker extension {} error: {}", id, e),
            Err(e) => println!("tracker extension task panicked: {}", e),
        }
    }
    all_results
}

#[tauri::command]
async fn search_nyaa_filtered(
    ext_manager: State<'_, Arc<ExtensionManager>>,
    query: String,
    season: Option<u32>,
    episode: Option<u32>,
    _is_movie: bool,
    media_type: Option<String>, // "anime", "tv", "movie"
    tracker_preference: Option<Vec<String>>, // extension ids, or None/[] for auto
    imdb_id: Option<String>, // For trackers that support IMDB lookup
) -> Result<Vec<search::SearchResult>, String> {
    println!("search_nyaa_filtered called with tracker_preference: {:?}, imdb_id: {:?}", tracker_preference, imdb_id);

    // Normalize query
    let normalized_query = query
        .replace("-", " ")
        .replace(":", " ")
        .replace("_", " ");

    let is_auto_mode = match &tracker_preference {
        Some(prefs) => prefs.is_empty(),
        None => true,
    };
    let is_anime = media_type.as_deref() == Some("anime");

    let all_trackers = ext_manager.enabled_trackers().await;

    let selected: Vec<extensions::LoadedExtension> = if is_auto_mode {
        // Auto mode: anime → anime trackers, everything else → general trackers
        all_trackers
            .iter()
            .filter(|e| e.manifest.is_anime == Some(is_anime))
            .cloned()
            .collect()
    } else {
        let prefs = tracker_preference.unwrap_or_default();
        all_trackers
            .iter()
            .filter(|e| prefs.contains(&e.id))
            .cloned()
            .collect()
    };

    println!(
        "Using tracker extensions: {:?}",
        selected.iter().map(|e| e.id.as_str()).collect::<Vec<_>>()
    );

    let mut all_results = search_tracker_extensions(
        selected,
        normalized_query.clone(),
        media_type.clone(),
        imdb_id.clone(),
        season,
        episode,
    )
    .await;

    if is_auto_mode && is_anime && all_results.is_empty() {
        println!("Anime search returned no results, falling back to regular trackers");
        let fallback: Vec<extensions::LoadedExtension> = all_trackers
            .iter()
            .filter(|e| e.manifest.is_anime == Some(false))
            .cloned()
            .collect();
        all_results = search_tracker_extensions(
            fallback,
            normalized_query.clone(),
            media_type.clone(),
            imdb_id.clone(),
            season,
            episode,
        )
        .await;
    }

    println!("Total results before deduplication: {}", all_results.len());

    let mut seen_hashes = std::collections::HashSet::new();
    all_results.retain(|result| {
        if let Some(hash) = extract_info_hash(&result.magnet_link) {
            seen_hashes.insert(hash)
        } else {
            true
        }
    });

    println!("Total results after deduplication: {}", all_results.len());
    Ok(all_results)
}

// Extract info hash from magnet link for deduplication
fn extract_info_hash(magnet: &str) -> Option<String> {
    // Strip the scheme so `xt=` matches even as the first parameter
    // ("magnet:?xt=urn:btih:...").
    let params = magnet.split_once('?').map(|(_, q)| q).unwrap_or(magnet);
    params
        .split('&')
        .find(|part| part.starts_with("xt=urn:btih:"))
        .and_then(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|hash| hash.to_lowercase())
}

#[tauri::command]
async fn save_torrent_selection(
    tracking: State<'_, TrackingManager>,
    show_id: u32,
    season: u32,
    episode: u32,
    magnet_link: String,
    file_index: usize,
) -> Result<(), String> {
    tracking
        .save_selection(show_id, season, episode, magnet_link, file_index)
        .await;
    Ok(())
}

#[tauri::command]
async fn save_multiple_torrent_selections(
    tracking: State<'_, TrackingManager>,
    show_id: u32,
    selections: Vec<(u32, u32, String, usize)>,
) -> Result<(), String> {
    tracking
        .save_multiple_selections(show_id, selections)
        .await;
    Ok(())
}

#[tauri::command]
async fn get_saved_selection(
    tracking: State<'_, TrackingManager>,
    #[allow(non_snake_case)] showId: u32,
    season: u32,
    episode: u32,
) -> Result<Option<tracking::EpisodeTorrent>, String> {
    Ok(tracking.get_selection(showId, season, episode).await)
}

#[tauri::command]
async fn get_all_torrent_selections(
    tracking: State<'_, TrackingManager>,
    #[allow(non_snake_case)] showId: u32,
) -> Result<Option<tracking::ShowHistory>, String> {
    Ok(tracking.get_all_selections(showId).await)
}

#[tauri::command]
async fn remove_saved_selection(
    tracking: State<'_, TrackingManager>,
    show_id: u32,
    season: u32,
    episode: u32,
) -> Result<(), String> {
    tracking.remove_selection(show_id, season, episode).await;
    Ok(())
}

#[tauri::command]
async fn remove_torrent_all_assignments(
    tracking: State<'_, TrackingManager>,
    show_id: u32,
    magnet_link: String,
) -> Result<(), String> {
    tracking.remove_all_by_magnet(show_id, magnet_link).await;
    Ok(())
}

#[tauri::command]
async fn check_is_anime(
    anime_list: State<'_, Arc<AnimeListManager>>,
    tmdb_id: u32,
) -> Result<bool, String> {
    Ok(anime_list.is_anime(tmdb_id).await)
}

#[tauri::command]
async fn refresh_anime_list(
    anime_list: State<'_, Arc<AnimeListManager>>,
) -> Result<(), String> {
    anime_list.refresh().await
}

#[tauri::command]
async fn get_http_port(manager: State<'_, Arc<TorrentManager>>) -> Result<u16, String> {
    manager.get_http_port().await
}

#[tauri::command]
async fn add_watch_history_item(
    watch_history: State<'_, WatchHistoryManager>,
    item: WatchHistoryItem,
) -> Result<(), String> {
    watch_history.add_item(item).await;
    Ok(())
}

#[tauri::command]
async fn get_watch_history(
    watch_history: State<'_, WatchHistoryManager>,
) -> Result<Vec<WatchHistoryItem>, String> {
    Ok(watch_history.get_history().await)
}

#[tauri::command]
async fn remove_watch_history_item(
    watch_history: State<'_, WatchHistoryManager>,
    media_id: u32,
    media_type: String,
) -> Result<(), String> {
    watch_history.remove_item(media_id, media_type).await;
    Ok(())
}

#[tauri::command]
async fn clear_watch_history(
    watch_history: State<'_, WatchHistoryManager>,
) -> Result<(), String> {
    watch_history.clear().await;
    Ok(())
}

#[tauri::command]
async fn get_my_list(
    my_list: State<'_, MyListManager>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(my_list.get_list().await)
}

#[tauri::command]
async fn set_my_list(
    my_list: State<'_, MyListManager>,
    items: Vec<serde_json::Value>,
) -> Result<(), String> {
    my_list.set_list(items).await;
    Ok(())
}

#[tauri::command]
async fn toggle_my_list_item(
    my_list: State<'_, MyListManager>,
    item: serde_json::Value,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(my_list.toggle_item(item).await)
}

#[tauri::command]
async fn get_watch_progress(
    watch_progress: State<'_, WatchProgressManager>,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    Ok(watch_progress.get_all().await)
}

#[tauri::command]
async fn set_watch_progress(
    watch_progress: State<'_, WatchProgressManager>,
    progress: std::collections::HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    watch_progress.set_all(progress).await;
    Ok(())
}

#[tauri::command]
async fn update_watch_progress_entry(
    watch_progress: State<'_, WatchProgressManager>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    watch_progress.update_entry(key, value).await;
    Ok(())
}

#[tauri::command]
async fn remove_watch_progress_entry(
    watch_progress: State<'_, WatchProgressManager>,
    key: String,
) -> Result<(), String> {
    watch_progress.remove_entry(key).await;
    Ok(())
}

#[tauri::command]
async fn clear_watch_progress(
    watch_progress: State<'_, WatchProgressManager>,
) -> Result<(), String> {
    watch_progress.clear().await;
    Ok(())
}

#[tauri::command]
async fn save_track_preference(
    track_prefs: State<'_, TrackPreferencesManager>,
    magnet_link: String,
    #[allow(non_snake_case)] audioTrackId: Option<i64>,
    #[allow(non_snake_case)] subtitleTrackId: Option<i64>,
    subtitle_language: Option<String>,
    subtitle_offset: Option<f64>,
) -> Result<(), String> {
    track_prefs.save_preference(magnet_link, audioTrackId, subtitleTrackId, subtitle_language, subtitle_offset).await;
    Ok(())
}

#[tauri::command]
async fn get_track_preference(
    track_prefs: State<'_, TrackPreferencesManager>,
    magnet_link: String,
) -> Result<Option<track_preferences::TrackPreference>, String> {
    Ok(track_prefs.get_preference(&magnet_link).await)
}

#[tauri::command]
async fn save_settings(
    settings_manager: State<'_, SettingsManager>,
    settings: Settings,
) -> Result<(), String> {
    settings_manager.save(settings).await;
    Ok(())
}

#[tauri::command]
async fn get_settings(
    settings_manager: State<'_, SettingsManager>,
) -> Result<Settings, String> {
    Ok(settings_manager.get().await)
}

#[tauri::command]
async fn check_external_player(player: String) -> Result<bool, String> {
    use std::process::Command;
    
    let command_name = match player.to_lowercase().as_str() {
        "mpv" => "mpv",
        "vlc" => if cfg!(target_os = "windows") { "vlc" } else { "vlc" },
        _ => return Err(format!("Unsupported player: {}", player)),
    };
    
    // On Windows, check common VLC installation paths
    #[cfg(target_os = "windows")]
    if player.to_lowercase() == "vlc" {
        use std::path::Path;
        let common_paths = vec![
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
        ];
        
        for path in common_paths {
            if Path::new(path).exists() {
                return Ok(true);
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    let check_result = Command::new("where")
        .arg(command_name)
        .creation_flags(0x08000000)
        .output();
    
    #[cfg(not(target_os = "windows"))]
    let check_result = Command::new("which")
        .arg(command_name)
        .output();
    
    match check_result {
        Ok(output) => Ok(output.status.success()),
        Err(_) => Ok(false),
    }
}

#[tauri::command]
async fn open_in_external_player(
    player: String,
    stream_url: String,
    title: String,
) -> Result<(), String> {
    use std::process::Command;
    
    let command_name = match player.to_lowercase().as_str() {
        "mpv" => "mpv".to_string(),
        "vlc" => {
            // On Windows, try to find VLC in common installation paths
            #[cfg(target_os = "windows")]
            {
                use std::path::Path;
                let common_paths = vec![
                    r"C:\Program Files\VideoLAN\VLC\vlc.exe",
                    r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
                ];
                
                common_paths.iter()
                    .find(|path| Path::new(path).exists())
                    .map(|path| path.to_string())
                    .unwrap_or_else(|| "vlc".to_string())
            }
            #[cfg(not(target_os = "windows"))]
            "vlc".to_string()
        },
        _ => return Err(format!("Unsupported player: {}", player)),
    };
    
    let mut cmd = Command::new(&command_name);
    
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    
    // Add player-specific arguments
    match player.to_lowercase().as_str() {
        "mpv" => {
            cmd.arg(&stream_url)
                .arg(format!("--title={}", title))
                .arg("--force-window=immediate");
        },
        "vlc" => {
            cmd.arg(&stream_url)
                .arg(format!("--meta-title={}", title));
        },
        _ => return Err(format!("Unsupported player: {}", player)),
    }
    
    // Spawn the process
    cmd.spawn()
        .map_err(|e| format!("Failed to launch {}: {}", player, e))?;
    
    Ok(())
}

#[tauri::command]
async fn download_update(url: String, _app_handle: tauri::AppHandle) -> Result<String, String> {
    let temp_dir = std::env::temp_dir();
    let file_name = url.split('/').last().unwrap_or("magnolia-installer.exe");
    let dest_path = temp_dir.join(file_name);

    println!("downloading update from: {}", url);
    println!("saving to: {:?}", dest_path);

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("failed to download: {}", e))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read response: {}", e))?;

    std::fs::write(&dest_path, bytes)
        .map_err(|e| format!("failed to write file: {}", e))?;

    println!("download complete: {:?}", dest_path);
    Ok(dest_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn install_update(installer_path: String, _app_handle: tauri::AppHandle) -> Result<(), String> {
    println!("running installer: {}", installer_path);

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;

        // Run NSIS installer with /S flag for silent install
        let status = Command::new(&installer_path)
            .arg("/S")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .status()
            .map_err(|e| format!("failed to run installer: {}", e))?;

        if !status.success() {
            return Err("installer failed".to_string());
        }

        // Delete the installer after successful install
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let _ = std::fs::remove_file(&installer_path);
        println!("installer removed: {}", installer_path);

        // Exit the app so the new version can start
        std::process::exit(0);
    }

    #[cfg(target_os = "macos")]
    {
        // Hands off to a detached helper script and exits — never returns on success.
        updater::install_macos(installer_path)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("auto-update not supported on this platform".to_string())
    }
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", &url])
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| format!("failed to open URL: {}", e))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("failed to open URL: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("failed to open URL: {}", e))?;
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::extract_info_hash;

    #[test]
    fn extract_info_hash_handles_leading_scheme() {
        // hash as the first magnet parameter (the common case)
        assert_eq!(
            extract_info_hash("magnet:?xt=urn:btih:ABCDEF123456&dn=Some%20Title&tr=udp%3A%2F%2Ffoo"),
            Some("abcdef123456".to_string())
        );
        // hash not first
        assert_eq!(
            extract_info_hash("magnet:?dn=Some%20Title&xt=urn:btih:ABCDEF123456"),
            Some("abcdef123456".to_string())
        );
        assert_eq!(extract_info_hash("magnet:?dn=No%20Hash"), None);
    }
}

fn main() {
    // Default: info for our code, warn for librqbit peer churn noise.
    // Override with RUST_LOG env var, e.g. RUST_LOG=debug or RUST_LOG=info,librqbit=error
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,librqbit=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init()
        .ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_handle = app.handle();
            let app_data_dir = app_handle
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");

            // Create app data dir if it doesn't exist
            if !app_data_dir.exists() {
                std::fs::create_dir_all(&app_data_dir).expect("failed to create app data dir");
            }

            let tracking_manager = TrackingManager::new(app_data_dir.clone());
            app.manage(tracking_manager);

            let watch_history_manager = WatchHistoryManager::new(app_data_dir.clone());
            app.manage(watch_history_manager);

            let my_list_manager = MyListManager::new(app_data_dir.clone());
            app.manage(my_list_manager);

            let watch_progress_manager = WatchProgressManager::new(app_data_dir.clone());
            app.manage(watch_progress_manager);

            let anime_list_manager = Arc::new(AnimeListManager::new(app_data_dir.clone()));
            app.manage(anime_list_manager.clone());
            let anime_refresh = anime_list_manager.clone();
            tauri::async_runtime::spawn(async move {
                anime_refresh.ensure_fresh().await;
            });

            let track_preferences_manager = TrackPreferencesManager::new(app_data_dir.clone());
            app.manage(track_preferences_manager);

            let settings_manager = SettingsManager::new(app_data_dir.clone());
            app.manage(settings_manager);

            let extension_manager = Arc::new(ExtensionManager::new(app_data_dir.clone()));
            app.manage(extension_manager);

            let logger = Logger::new(&app_handle)
                .expect("failed to create logger");
            app.manage(logger);

            let cache_metadata_manager = CacheMetadataManager::new(&app_handle)
                .expect("failed to create cache metadata manager");
            app.manage(std::sync::Mutex::new(cache_metadata_manager));

            let torrent_dir = app_data_dir.join("torrents");
            let torrent_manager = tauri::async_runtime::block_on(async {
                TorrentManager::new(torrent_dir)
                    .await
                    .expect("Failed to initialize torrent manager")
            });
            let torrent_manager_arc = Arc::new(torrent_manager);
            app.manage(torrent_manager_arc.clone());

            let main_window = app.get_webview_window("main").unwrap();

            // Set macOS-specific window properties for inset traffic lights
            #[cfg(target_os = "macos")]
            {
                use tauri::TitleBarStyle;
                let _ = main_window.set_title_bar_style(TitleBarStyle::Overlay);
            }

            // ── initialise libmpv and embed it under the WebView ──────────
            {
                use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

                let wh = main_window
                    .window_handle()
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;

                let dh = main_window
                    .display_handle()
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;

                let (raw_window_ptr, display_ptr): (
                    *const std::ffi::c_void,
                    Option<*const std::ffi::c_void>,
                ) = match wh.as_raw() {
                    #[cfg(target_os = "macos")]
                    RawWindowHandle::AppKit(h) => {
                        (h.ns_view.as_ptr() as *const std::ffi::c_void, None)
                    }
                    #[cfg(target_os = "windows")]
                    RawWindowHandle::Win32(h) => {
                        (h.hwnd.get() as *const std::ffi::c_void, None)
                    }
                    #[cfg(target_os = "linux")]
                    RawWindowHandle::Xcb(h) => {
                        let connection = match dh.as_raw() {
                            RawDisplayHandle::Xcb(d) => {
                                d.connection.map(|p| p.as_ptr() as *const std::ffi::c_void)
                            }
                            _ => None,
                        };
                        (h.window.get() as *const std::ffi::c_void, connection)
                    }
                    #[cfg(target_os = "linux")]
                    RawWindowHandle::Wayland(h) => {
                        (h.surface.as_ptr() as *const std::ffi::c_void, None)
                    }
                    _ => return Err("unsupported platform window handle".into()),
                };

                let mpv_handle =
                    mpv::MpvHandle::new(raw_window_ptr, display_ptr, app_handle.clone())
                        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

                let mpv_arc = Arc::new(Mutex::new(mpv_handle));
                // Register AppState BEFORE starting the event listener —
                // the event loop accesses AppState via app_handle.state::<AppState>().
                app.manage(AppState {
                    mpv_player: mpv_arc.clone(),
                });

                let guard = mpv_arc
                    .lock()
                    .map_err(|e| -> Box<dyn std::error::Error> { format!("mpv lock: {e}").into() })?;
                guard.start_event_listener();
                guard.start_render_loop();

                // ── resize the embedded mpv NSView whenever the window resizes ──
                let mpv_for_resize = mpv_arc.clone();
                let window_for_resize = main_window.clone();
                main_window.on_window_event(move |event| {
                    let physical_size = match event {
                        tauri::WindowEvent::Resized(s) => *s,
                        tauri::WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                            *new_inner_size
                        }
                        _ => return,
                    };
                    let scale = window_for_resize.scale_factor().unwrap_or(2.0);
                    let pw = physical_size.width.max(1);
                    let ph = physical_size.height.max(1);
                    let logical_w = pw as f64 / scale;
                    let logical_h = ph as f64 / scale;
                    if let Ok(guard) = mpv_for_resize.lock() {
                        guard.resize_view(logical_w, logical_h, scale);
                        #[cfg(target_os = "macos")]
                        let utils_usize = guard.soia_utils_ptr() as usize;
                        drop(guard);
                        #[cfg(target_os = "macos")]
                        {
                            let window_cl = window_for_resize.clone();
                            let app = window_for_resize.app_handle().clone();
                            let _ = app.run_on_main_thread(move || {
                                if let Ok(ns_view) = window_cl.ns_view() {
                                    unsafe {
                                        crate::mpv::ffi::soia_sync_layer_geometry(
                                            ns_view as *mut std::ffi::c_void,
                                            utils_usize,
                                        );
                                    }
                                }
                            });
                        }
                    }
                });

                // ── initial geometry sync at startup so video layer is visible immediately ──
                #[cfg(target_os = "macos")]
                {
                    if let Ok(physical_size) = main_window.inner_size() {
                        let scale = main_window.scale_factor().unwrap_or(2.0);
                        let pw = physical_size.width.max(1);
                        let ph = physical_size.height.max(1);
                        let logical_w = pw as f64 / scale;
                        let logical_h = ph as f64 / scale;
                        guard.resize_view(logical_w, logical_h, scale);
                    }
                    if let Ok(ns_view) = main_window.ns_view() {
                        let utils_usize = guard.soia_utils_ptr() as usize;
                        unsafe {
                            crate::mpv::ffi::soia_sync_layer_geometry(
                                ns_view as *mut std::ffi::c_void,
                                utils_usize,
                            );
                        }
                    }
                }
            }

            // Cleanup on app close
            let manager_for_cleanup = torrent_manager_arc.clone();
            main_window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    tauri::async_runtime::block_on(async {
                        if let Err(e) = manager_for_cleanup.cleanup_all().await {
                            eprintln!("Error during cleanup: {}", e);
                        }
                    });
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            torrent::add_torrent,
            torrent::get_torrent_info,
            torrent::list_torrents,
            torrent::prepare_stream,
            torrent::get_stream_status,
            torrent::get_torrent_piece_ranges,
            torrent::stop_stream,
            torrent::wipe_all_torrent_files,
            torrent::pause_torrent,
            torrent::resume_torrent,
            torrent::remove_torrent,
            torrent::get_download_dir,
            search_nyaa_filtered,
            save_torrent_selection,
            save_multiple_torrent_selections,
            get_saved_selection,
            get_all_torrent_selections,
            remove_saved_selection,
            remove_torrent_all_assignments,
            check_is_anime,
            refresh_anime_list,
            get_http_port,
            add_watch_history_item,
            get_watch_history,
            remove_watch_history_item,
            clear_watch_history,
            get_my_list,
            set_my_list,
            toggle_my_list_item,
            get_watch_progress,
            set_watch_progress,
            update_watch_progress_entry,
            remove_watch_progress_entry,
            clear_watch_progress,
            save_track_preference,
            get_track_preference,
            save_settings,
            get_settings,
            check_external_player,
            open_in_external_player,
            logger::log_message,
            cache_metadata::save_cache_metadata,
            cache_metadata::get_cache_metadata,
            cache_metadata::get_all_cache_metadata,
            download_update,
            install_update,
            updater::get_platform_info,
            updater::enable_touch_id_sudo,
            updater::is_touch_id_sudo_enabled,
            open_external_url,
            subtitles::fetch_subtitles,
            subtitles::download_subtitle,
            extensions::list_extensions,
            extensions::install_extension_from_path,
            extensions::install_extension_from_url,
            extensions::remove_extension,
            extensions::set_extension_enabled,
            extensions::set_extension_field_values,
            extensions::fetch_extension_subtitles,
            subtitle_packs::import_subtitle_pack,
            subtitle_packs::get_subtitle_pack_for_episode,
            subtitle_packs::get_subtitle_pack_coverage,
            subtitle_packs::remove_subtitle_pack_episode,
            subtitle_packs::clear_subtitle_pack,
            load_file,
            cycle_pause,
            seek_video,
            mpv_run_command,
            mpv_set_option_string
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
