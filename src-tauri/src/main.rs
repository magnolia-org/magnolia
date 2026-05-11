
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mpv;
mod search;
mod torrent;
mod tracking;
mod watch_history;
mod track_preferences;
mod settings;
mod logger;
mod cache_metadata;
mod subtitles;

use search::{nyaa::NyaaProvider, limetorrents::LimeTorrentsProvider, piratebay::PirateBayProvider,
             SearchProvider};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Manager, State};
use torrent::TorrentManager;
use tracking::TrackingManager;
use watch_history::{WatchHistoryManager, WatchHistoryItem};
use track_preferences::TrackPreferencesManager;
use settings::{SettingsManager, Settings};
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
    let guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    let start_opt;
    let mut args: Vec<&str> = vec!["loadfile", &path, "replace"];
    if let Some(pos) = resumePosition {
        if pos > 0.0 {
            start_opt = format!("start={pos}");
            args.push(&start_opt);
        }
    }
    guard.command(&args);
    let pause_val = if autoPlay.unwrap_or(true) { "no" } else { "yes" };
    guard.set_option_string("pause", pause_val);
    Ok(())
}

#[tauri::command]
async fn cycle_pause(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    if guard.eof_reached() {
        guard.command(&["seek", "0", "absolute"]);
        guard.set_option_string("pause", "no");
    } else {
        guard.command(&["cycle", "pause"]);
    }
    Ok(())
}

#[tauri::command]
async fn seek_video(state: State<'_, AppState>, seconds: f64) -> Result<(), String> {
    let guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    let pos_str = seconds.to_string();
    guard.command(&["seek", &pos_str, "absolute"]);
    if guard.eof_reached() {
        guard.set_option_string("pause", "no");
    }
    Ok(())
}

#[tauri::command]
async fn mpv_run_command(
    state: State<'_, AppState>,
    args: Vec<String>,
) -> Result<(), String> {
    let guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    guard.command(&refs);
    Ok(())
}

#[tauri::command]
async fn mpv_set_option_string(
    state: State<'_, AppState>,
    name: String,
    value: String,
) -> Result<(), String> {
    let guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    guard.set_option_string(&name, &value);
    Ok(())
}

// ── search commands ──────────────────────────────────────────────────────────

#[tauri::command]
async fn search_nyaa(query: String) -> Result<Vec<search::SearchResult>, String> {
    let provider = NyaaProvider::new();
    provider.search(&query).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_nyaa_filtered(
    query: String,
    _season: Option<u32>,
    _episode: Option<u32>,
    _is_movie: bool,
    media_type: Option<String>, // "anime", "tv", "movie"
    tracker_preference: Option<Vec<String>>, // ["nyaa", "limetorrents", ...] or None for auto
    imdb_id: Option<String>, // For EZTV: pass IMDB ID like "tt1234567" or "1234567"
) -> Result<Vec<search::SearchResult>, String> {
    println!("search_nyaa_filtered called with tracker_preference: {:?}, imdb_id: {:?}", tracker_preference, imdb_id);
    
    // Normalize query
    let normalized_query = query
        .replace("-", " ")
        .replace(":", " ")
        .replace("_", " ");
    
    // Determine if this is auto mode
    let is_auto_mode = match &tracker_preference {
        Some(prefs) => prefs.is_empty(),
        None => true,
    };
    
    let is_anime = media_type.as_deref() == Some("anime");
    
    let trackers: Vec<String> = if let Some(prefs) = tracker_preference {
        if prefs.is_empty() {
            match media_type.as_deref() {
                Some("anime") => vec!["nyaa".to_string()],
                _ => {
                    let mut t = vec!["limetorrents".to_string(), "thepiratebay".to_string()];
                    if imdb_id.is_some() {
                        t.push("eztv".to_string());
                    }
                    t
                }
            }
        } else {
            prefs
        }
    } else {
        // null/undefined means auto mode
        match media_type.as_deref() {
            Some("anime") => vec!["nyaa".to_string()],
            _ => {
                let mut t = vec!["limetorrents".to_string(), "thepiratebay".to_string()];
                if imdb_id.is_some() {
                    t.push("eztv".to_string());
                }
                t
            }
        }
    };
    
    println!("Using trackers: {:?}", trackers);
    
    // Helper function to search trackers
    async fn search_trackers(
        trackers: Vec<String>,
        query: String,
        imdb_id: Option<String>,
    ) -> Vec<search::SearchResult> {
        let mut handles = vec![];
        
        for tracker in trackers {
            let query_clone = query.clone();
            let imdb_clone = imdb_id.clone();
            
            let handle = tokio::spawn(async move {
                let result: Result<Vec<search::SearchResult>, Box<dyn std::error::Error + Send + Sync>> = match tracker.as_str() {
                    "nyaa" => {
                        println!("Searching Nyaa...");
                        NyaaProvider::new().search(&query_clone).await
                    }
                    "limetorrents" => {
                        println!("Searching LimeTorrents...");
                        LimeTorrentsProvider::new().search(&query_clone).await
                    }
                    "thepiratebay" => {
                        println!("Searching ThePirateBay...");
                        let provider = PirateBayProvider::new();
                        if let Some(ref imdb) = imdb_clone {
                            provider.search_with_imdb(&query_clone, Some(imdb)).await
                        } else {
                            provider.search(&query_clone).await
                        }
                    }
                    "eztv" => {
                        if let Some(ref imdb) = imdb_clone {
                            println!("Searching EZTV with IMDB ID: {}", imdb);
                            search::eztv::EZTVProvider::new().search_by_imdb(imdb).await
                        } else {
                            println!("EZTV requires IMDB ID, skipping");
                            Ok(vec![])
                        }
                    }
                    _ => {
                        println!("Unknown tracker: {}", tracker);
                        Ok(vec![])
                    }
                };
                
                match result {
                    Ok(results) => {
                        println!("{} returned {} results", tracker, results.len());
                        results
                    }
                    Err(e) => {
                        println!("{} error: {}", tracker, e);
                        vec![]
                    }
                }
            });
            handles.push(handle);
        }
        
        let mut all_results = Vec::new();
        for handle in handles {
            if let Ok(results) = handle.await {
                all_results.extend(results);
            }
        }
        all_results
    }
    
    let mut all_results = search_trackers(trackers, normalized_query.clone(), imdb_id.clone()).await;
    
    if is_auto_mode && is_anime && all_results.is_empty() {
        println!("Anime search returned no results, falling back to regular trackers");
        let mut fallback_trackers = vec!["limetorrents".to_string(), "thepiratebay".to_string()];
        if imdb_id.is_some() {
            fallback_trackers.push("eztv".to_string());
        }
        all_results = search_trackers(fallback_trackers, normalized_query.clone(), imdb_id.clone()).await;
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
    magnet
        .split('&')
        .find(|part| part.starts_with("xt=urn:btih:"))
        .and_then(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|hash| hash.to_lowercase())
}

#[tauri::command]
async fn search_eztv_by_imdb(imdb_id: String) -> Result<Vec<search::SearchResult>, String> {
    println!("Searching EZTV with IMDb ID: {}", imdb_id);
    let provider = search::eztv::EZTVProvider::new();
    provider.search_by_imdb(&imdb_id).await.map_err(|e| e.to_string())
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

    #[cfg(not(target_os = "windows"))]
    {
        Err("auto-update only supported on windows".to_string())
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

fn main() {
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

            let track_preferences_manager = TrackPreferencesManager::new(app_data_dir.clone());
            app.manage(track_preferences_manager);

            let settings_manager = SettingsManager::new(app_data_dir.clone());
            app.manage(settings_manager);

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
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};

                let wh = main_window
                    .window_handle()
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
                    RawWindowHandle::Xcb(h) => (
                        h.window.get() as *const std::ffi::c_void,
                        h.connection
                            .map(|p| p.as_ptr() as *const std::ffi::c_void),
                    ),
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
            torrent::stop_stream,
            torrent::wipe_all_torrent_files,
            torrent::pause_torrent,
            torrent::resume_torrent,
            torrent::remove_torrent,
            torrent::get_download_dir,
            search_nyaa,
            search_nyaa_filtered,
            search_eztv_by_imdb,
            save_torrent_selection,
            save_multiple_torrent_selections,
            get_saved_selection,
            get_all_torrent_selections,
            remove_saved_selection,
            get_http_port,
            add_watch_history_item,
            get_watch_history,
            remove_watch_history_item,
            clear_watch_history,
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
            open_external_url,
            subtitles::fetch_wyzie_subtitles,
            load_file,
            cycle_pause,
            seek_video,
            mpv_run_command,
            mpv_set_option_string
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
