
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod search;
mod torrent;
mod tracking;
mod dash;
mod media_cache;
mod font_manager;
mod watch_history;
mod track_preferences;
mod settings;

use search::{nyaa::NyaaProvider, limetorrents::LimeTorrentsProvider, piratebay::PirateBayProvider, 
             SearchProvider};
use std::sync::Arc;
use tauri::{Manager, State};
use torrent::TorrentManager;
use tracking::TrackingManager;
use media_cache::{MediaCache, TrackType};
use font_manager::FontManager;
use watch_history::{WatchHistoryManager, WatchHistoryItem};
use track_preferences::TrackPreferencesManager;
use settings::{SettingsManager, Settings};
use ffmpeg_sidecar::download::{check_latest_version, download_ffmpeg_package, unpack_ffmpeg};

// Check if ffmpeg is installed on system or via sidecar
fn is_ffmpeg_installed() -> bool {
    // First check if ffmpeg is in system PATH
    #[cfg(target_os = "windows")]
    let system_check = std::process::Command::new("where")
        .arg("ffmpeg")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    
    #[cfg(not(target_os = "windows"))]
    let system_check = std::process::Command::new("which")
        .arg("ffmpeg")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    
    if system_check {
        println!("ffmpeg found in system PATH");
        return true;
    }
    
    // Check if sidecar ffmpeg exists
    let sidecar_exists = ffmpeg_sidecar::paths::ffmpeg_path().exists();
    if sidecar_exists {
        println!("ffmpeg found in sidecar directory");
    }
    
    sidecar_exists
}

// Check and install ffmpeg if not present
async fn ensure_ffmpeg_installed() -> Result<(), Box<dyn std::error::Error>> {
    if is_ffmpeg_installed() {
        println!("ffmpeg is already available");
        return Ok(());
    }
    
    println!("============================================");
    println!("ffmpeg not found, downloading and installing...");
    println!("this may take a few minutes (approximately 80MB download)");
    println!("============================================");
    
    // Get sidecar directory first
    let sidecar_dir = match ffmpeg_sidecar::paths::sidecar_dir() {
        Ok(dir) => {
            println!("sidecar directory: {:?}", dir);
            dir
        }
        Err(e) => {
            eprintln!("failed to get sidecar directory: {}", e);
            return Err(e.into());
        }
    };
    
    // Create sidecar directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&sidecar_dir) {
        eprintln!("failed to create sidecar directory: {}", e);
        return Err(e.into());
    }
    
    // Download ffmpeg
    println!("checking latest version...");
    let download_url = match check_latest_version() {
        Ok(url) => {
            println!("download URL: {}", url);
            url
        }
        Err(e) => {
            eprintln!("failed to check latest version: {}", e);
            return Err(e.into());
        }
    };
    
    let destination = sidecar_dir.join("ffmpeg-download.zip");
    println!("downloading to: {:?}", destination);
    
    if let Err(e) = download_ffmpeg_package(&download_url, &destination) {
        eprintln!("failed to download ffmpeg package: {}", e);
        return Err(e.into());
    }
    
    println!("download complete, unpacking...");
    
    if let Err(e) = unpack_ffmpeg(&destination, &sidecar_dir) {
        eprintln!("failed to unpack ffmpeg: {}", e);
        // Clean up partial download
        let _ = std::fs::remove_file(&destination);
        return Err(e.into());
    }
    
    // Clean up downloaded archive
    let _ = std::fs::remove_file(&destination);
    
    println!("ffmpeg installed successfully to {:?}", sidecar_dir);
    
    // Verify installation
    let ffmpeg_exe = ffmpeg_sidecar::paths::ffmpeg_path();
    if ffmpeg_exe.exists() {
        println!("ffmpeg installation verified at: {:?}", ffmpeg_exe);
        println!("============================================");
        Ok(())
    } else {
        eprintln!("ffmpeg installation failed - binary not found after unpacking");
        eprintln!("expected location: {:?}", ffmpeg_exe);
        Err("ffmpeg installation failed - binary not found after unpacking".into())
    }
}

#[tauri::command]
async fn search_nyaa(query: String) -> Result<Vec<search::SearchResult>, String> {
    let provider = NyaaProvider::new();
    provider.search(&query).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_nyaa_filtered(
    query: String,
    season: Option<u32>,
    episode: Option<u32>,
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
    
    // Determine which trackers to use
    let trackers: Vec<String> = if let Some(prefs) = tracker_preference {
        if prefs.is_empty() {
            // Empty array means auto mode
            match media_type.as_deref() {
                Some("anime") => vec!["nyaa".to_string()],
                // For regular TV/movies: use limetorrents, thepiratebay, and eztv (if imdb available)
                _ => {
                    let mut t = vec!["limetorrents".to_string(), "thepiratebay".to_string()];
                    if imdb_id.is_some() {
                        t.push("eztv".to_string());
                    }
                    t
                }
            }
        } else {
            // Use specified trackers
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
    
    // Search with primary trackers
    let mut all_results = search_trackers(trackers, normalized_query.clone(), imdb_id.clone()).await;
    
    // If anime auto mode returned no results, fallback to regular trackers
    if is_auto_mode && is_anime && all_results.is_empty() {
        println!("Anime search returned no results, falling back to regular trackers");
        let mut fallback_trackers = vec!["limetorrents".to_string(), "thepiratebay".to_string()];
        if imdb_id.is_some() {
            fallback_trackers.push("eztv".to_string());
        }
        all_results = search_trackers(fallback_trackers, normalized_query.clone(), imdb_id.clone()).await;
    }
    
    println!("Total results before deduplication: {}", all_results.len());
    
    // Deduplicate by info hash (from magnet link)
    let mut seen_hashes = std::collections::HashSet::new();
    all_results.retain(|result| {
        if let Some(hash) = extract_info_hash(&result.magnet_link) {
            seen_hashes.insert(hash)
        } else {
            true // Keep if can't extract hash
        }
    });
    
    println!("Total results after deduplication: {}", all_results.len());
    
    // Don't filter out results - just sort by relevance score
    // This allows all EZTV results (and others) to be shown
    // Matching season/episode will be prioritized via scoring

    // Sort by relevance score
    all_results.sort_by(|a, b| {
        let score_a = calculate_relevance_score(a, season, episode, &normalized_query);
        let score_b = calculate_relevance_score(b, season, episode, &normalized_query);
        match score_b.cmp(&score_a) {
            std::cmp::Ordering::Equal => b.seeds.cmp(&a.seeds),
            other => other,
        }
    });

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

fn calculate_relevance_score(
    result: &search::SearchResult,
    requested_season: Option<u32>,
    requested_episode: Option<u32>,
    query: &str,
) -> i32 {
    let mut score = 0;

    // Exact season/episode match is highly relevant
    if let (Some(req_s), Some(req_e)) = (requested_season, requested_episode) {
        if let Some(s) = result.season {
            if s == req_s {
                score += 100; // Correct season
                
                if let Some(e) = result.episode {
                    if e == req_e {
                        score += 100; // Exact episode match - highest priority
                    } else {
                        score -= 50; // Wrong episode in correct season
                    }
                } else if result.is_batch {
                    score += 50; // Batch for correct season - good fallback
                }
            } else {
                score -= 100; // Wrong season
            }
        }
    }

    // Quality detection adds relevance
    if result.quality.is_some() {
        score += 10;
        
        // Prefer 1080p
        if let Some(ref quality) = result.quality {
            if quality.contains("1080") {
                score += 15;
            } else if quality.contains("720") {
                score += 10;
            } else if quality.contains("2160") || quality.contains("4K") {
                score += 5; // 4K is good but might be too large
            }
        }
    }

    // Encode detection adds relevance
    if result.encode.is_some() {
        score += 5;
        
        // Prefer modern codecs
        if let Some(ref encode) = result.encode {
            if encode.contains("265") || encode.contains("HEVC") {
                score += 10; // Modern efficient codec
            } else if encode.contains("264") || encode.contains("AVC") {
                score += 5; // Standard codec
            }
        }
    }

    // Title similarity to query (simple word matching)
    let title_lower = result.title.to_lowercase();
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let matched_words = query_words.iter().filter(|word| title_lower.contains(*word)).count();
    score += (matched_words * 5) as i32;

    // Seed count contributes to relevance (but less than exact matches)
    score += (result.seeds.min(100) / 10) as i32; // Max 10 points from seeds

    // Penalize if it's marked as batch when we want a specific episode
    if requested_episode.is_some() && result.is_batch && result.episode.is_none() {
        score -= 30; // Batch without specific episode when we want one episode
    }

    score
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
async fn get_saved_selection(
    tracking: State<'_, TrackingManager>,
    #[allow(non_snake_case)] showId: u32,
    season: u32,
    episode: u32,
) -> Result<Option<tracking::EpisodeTorrent>, String> {
    Ok(tracking.get_selection(showId, season, episode).await)
}

#[tauri::command]
async fn save_subtitle_cache(
    cache: State<'_, MediaCache>,
    cache_id: String,
    file_index: usize,
    track_index: usize,
    data: String,
) -> Result<(), String> {
    cache.save_track(TrackType::Subtitle, &cache_id, file_index, track_index, data.into_bytes()).await
}

#[tauri::command]
async fn load_subtitle_cache(
    cache: State<'_, MediaCache>,
    cache_id: String,
    file_index: usize,
    track_index: usize,
) -> Result<Option<String>, String> {
    let result = cache.load_track(TrackType::Subtitle, &cache_id, file_index, track_index).await?;
    Ok(result.and_then(|bytes| String::from_utf8(bytes).ok()))
}

#[tauri::command]
async fn clear_subtitle_cache(
    cache: State<'_, MediaCache>,
) -> Result<(), String> {
    cache.clear_cache(TrackType::Subtitle).await
}

#[tauri::command]
async fn save_audio_cache(
    cache: State<'_, MediaCache>,
    cache_id: String,
    file_index: usize,
    track_index: usize,
    data: String,
) -> Result<(), String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let bytes = STANDARD.decode(&data).map_err(|e| format!("Failed to decode base64: {}", e))?;
    cache.save_track(TrackType::Audio, &cache_id, file_index, track_index, bytes).await
}

#[tauri::command]
async fn load_audio_cache(
    cache: State<'_, MediaCache>,
    cache_id: String,
    file_index: usize,
    track_index: usize,
) -> Result<Option<String>, String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let data = cache.load_track(TrackType::Audio, &cache_id, file_index, track_index).await?;
    Ok(data.map(|bytes| STANDARD.encode(&bytes)))
}

#[tauri::command]
async fn clear_audio_cache(
    cache: State<'_, MediaCache>,
) -> Result<(), String> {
    cache.clear_cache(TrackType::Audio).await
}

#[tauri::command]
async fn load_transcoded_audio(
    torrent_manager: State<'_, Arc<torrent::TorrentManager>>,
    session_id: usize,
    file_index: usize,
) -> Result<Option<Vec<u8>>, String> {
    torrent_manager.get_transcoded_audio(session_id, file_index).await
}

#[tauri::command]
async fn save_font(
    font_manager: State<'_, FontManager>,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    // Check if font is already installed on system
    if font_manager::is_font_installed(&filename) {
        println!("Font {} is already installed on system, skipping save", filename);
        return Ok(format!("system:{}", filename));
    }
    
    let path = font_manager.save_font(&filename, &data)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn check_font_installed(filename: String) -> bool {
    font_manager::is_font_installed(&filename)
}

#[tauri::command]
fn list_fonts(font_manager: State<'_, FontManager>) -> Result<Vec<font_manager::FontInfo>, String> {
    font_manager.list_fonts()
}

#[tauri::command]
fn get_fonts_dir(font_manager: State<'_, FontManager>) -> String {
    font_manager.get_fonts_dir().to_string_lossy().to_string()
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
    audio_track_index: Option<usize>,
    subtitle_track_index: Option<i32>,
    subtitle_language: Option<String>,
) -> Result<(), String> {
    track_prefs.save_preference(magnet_link, audio_track_index, subtitle_track_index, subtitle_language).await;
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

fn main() {
    // Ensure ffmpeg is installed before starting the app
    println!("checking ffmpeg installation...");
    tauri::async_runtime::block_on(async {
        match ensure_ffmpeg_installed().await {
            Ok(_) => println!("ffmpeg ready"),
            Err(e) => {
                eprintln!("============================================");
                eprintln!("WARNING: Failed to install ffmpeg: {}", e);
                eprintln!("Audio transcoding features may not work properly");
                eprintln!("You can manually install ffmpeg to your system PATH");
                eprintln!("============================================");
            }
        }
    });
    
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

            let media_cache = MediaCache::new(app_data_dir.clone());
            app.manage(media_cache);

            let watch_history_manager = WatchHistoryManager::new(app_data_dir.clone());
            app.manage(watch_history_manager);

            let track_preferences_manager = TrackPreferencesManager::new(app_data_dir.clone());
            app.manage(track_preferences_manager);

            let settings_manager = SettingsManager::new(app_data_dir.clone());
            app.manage(settings_manager);

            let font_manager = FontManager::new(&app_handle)
                .expect("failed to create font manager");
            app.manage(font_manager);

            let torrent_dir = app_data_dir.join("torrents");
            let torrent_manager = tauri::async_runtime::block_on(async {
                TorrentManager::new(torrent_dir)
                    .await
                    .expect("Failed to initialize torrent manager")
            });
            let torrent_manager_arc = Arc::new(torrent_manager);
            app.manage(torrent_manager_arc.clone());

            // Cleanup torrents on app close
            let manager_for_cleanup = torrent_manager_arc.clone();
            let main_window = app.get_webview_window("main").unwrap();
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
            torrent::pause_torrent,
            torrent::resume_torrent,
            torrent::remove_torrent,
            torrent::get_download_dir,
            torrent::extract_subtitle,
            torrent::extract_audio_track,
            search_nyaa,
            search_nyaa_filtered,
            search_eztv_by_imdb,
            save_torrent_selection,
            get_saved_selection,
            save_subtitle_cache,
            load_subtitle_cache,
            clear_subtitle_cache,
            save_audio_cache,
            load_audio_cache,
            clear_audio_cache,
            load_transcoded_audio,
            save_font,
            check_font_installed,
            list_fonts,
            get_fonts_dir,
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
            open_in_external_player
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

