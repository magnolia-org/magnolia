
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod mpv;
mod logger;
mod updater;

use magnolia_core::Core;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{Manager, State};
use logger::Logger;

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

#[tauri::command]
async fn check_external_player(player: String, custom_path: Option<String>) -> Result<bool, String> {
    use std::process::Command;

    // Custom player: verify the chosen program exists on disk or resolves in PATH.
    if player.to_lowercase() == "custom" {
        use std::path::Path;
        let path = custom_path.unwrap_or_default();
        let path = path.trim();
        if path.is_empty() {
            return Ok(false);
        }
        if Path::new(path).exists() {
            return Ok(true);
        }

        #[cfg(target_os = "windows")]
        let check_result = Command::new("where")
            .arg(path)
            .creation_flags(0x08000000)
            .output();

        #[cfg(not(target_os = "windows"))]
        let check_result = Command::new("which").arg(path).output();

        return Ok(check_result.map(|o| o.status.success()).unwrap_or(false));
    }

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
    custom_path: Option<String>,
) -> Result<(), String> {
    use std::process::Command;

    // Custom player: launch the user-chosen program with the stream URL.
    if player.to_lowercase() == "custom" {
        let path = custom_path.unwrap_or_default();
        let path = path.trim();
        if path.is_empty() {
            return Err("No custom player program selected".to_string());
        }

        // On macOS the user usually picks a `.app` bundle, which is a directory
        // and cannot be executed directly. Launch it through `open -a` instead.
        #[cfg(target_os = "macos")]
        if path.ends_with(".app") || std::path::Path::new(path).is_dir() {
            Command::new("open")
                .arg("-a")
                .arg(path)
                .arg(&stream_url)
                .spawn()
                .map_err(|e| format!("Failed to launch custom player: {}", e))?;
            return Ok(());
        }

        let mut cmd = Command::new(path);

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);

        cmd.arg(&stream_url);

        cmd.spawn()
            .map_err(|e| format!("Failed to launch custom player: {}", e))?;

        return Ok(());
    }

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

#[cfg(target_os = "linux")]
fn disable_webkit_60fps_cap(webview: &webkit2gtk::WebView) {
    use std::ffi::{c_char, c_void, CStr};
    use webkit2gtk::glib::translate::ToGlibPtr;
    use webkit2gtk::WebViewExt;

    type GetAllFeatures = unsafe extern "C" fn() -> *mut c_void;
    type FeatureListGetLength = unsafe extern "C" fn(*mut c_void) -> usize;
    type FeatureListGet = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type FeatureListUnref = unsafe extern "C" fn(*mut c_void);
    type FeatureGetIdentifier = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SetFeatureEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, i32);

    unsafe fn lookup<T>(name: &CStr) -> Option<T> {
        let ptr = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr());
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    let Some(settings) = WebViewExt::settings(webview) else {
        return;
    };
    let settings_ptr: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;

    unsafe {
        let (
            Some(get_all_features),
            Some(list_get_length),
            Some(list_get),
            Some(list_unref),
            Some(get_identifier),
            Some(set_feature_enabled),
        ) = (
            lookup::<GetAllFeatures>(c"webkit_settings_get_all_features"),
            lookup::<FeatureListGetLength>(c"webkit_feature_list_get_length"),
            lookup::<FeatureListGet>(c"webkit_feature_list_get"),
            lookup::<FeatureListUnref>(c"webkit_feature_list_unref"),
            lookup::<FeatureGetIdentifier>(c"webkit_feature_get_identifier"),
            lookup::<SetFeatureEnabled>(c"webkit_settings_set_feature_enabled"),
        )
        else {
            return;
        };

        let features = get_all_features();
        if features.is_null() {
            return;
        }
        for i in 0..list_get_length(features) {
            let feature = list_get(features, i);
            let identifier = get_identifier(feature);
            if !identifier.is_null()
                && CStr::from_ptr(identifier).to_bytes() == b"PreferPageRenderingUpdatesNear60FPSEnabled"
            {
                set_feature_enabled(settings_ptr.cast(), feature, 0);
                tracing::info!("disabled WebKit 60fps rendering cap");
            }
        }
        list_unref(features);
    }
}

fn main() {
    // NVIDIA's Wayland EGL driver enables explicit sync (wp_linux_drm_syncobj)
    // on the window's wl_surface; when GTK or the embedded mpv layer then
    // commits a non-dmabuf buffer on that surface, the compositor raises a
    // fatal protocol error ("Explicit Sync only supported on dmabuf buffers")
    // and GDK exits the process. Disable explicit sync for this process before
    // GTK/EGL initialise; the variable is ignored on non-NVIDIA systems.
    #[cfg(target_os = "linux")]
    if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }

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
        .plugin(tauri_plugin_os::init())
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

            let logger = Logger::new(&app_handle)
                .expect("failed to create logger");
            app.manage(logger);

            let core = tauri::async_runtime::block_on(Core::new(app_data_dir.clone()))
                .expect("failed to initialize magnolia core");
            let core = Arc::new(core);
            app.manage(core.clone());

            let anime_list = core.anime_list.clone();
            tauri::async_runtime::spawn(async move {
                anime_list.ensure_fresh().await;
            });

            let main_window = app.get_webview_window("main").unwrap();

            #[cfg(target_os = "linux")]
            let _ = main_window.with_webview(|webview| disable_webkit_60fps_cap(&webview.inner()));

            // Set macOS-specific window properties for inset traffic lights
            #[cfg(target_os = "macos")]
            {
                use tauri::TitleBarStyle;
                let _ = main_window.set_title_bar_style(TitleBarStyle::Overlay);
            }

            // ── initialise libmpv and embed it under the WebView ──────────
            {
                use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
                #[cfg(target_os = "linux")]
                use raw_window_handle::RawDisplayHandle;

                let wh = main_window
                    .window_handle()
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;

                let dh = main_window
                    .display_handle()
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;

                // soia_utils embed mode: macOS = 0 (native/Metal),
                // X11/HWND = 1 (wid), Wayland = 2 (render context).
                let (raw_window_ptr, display_ptr, embed_mode): (
                    *const std::ffi::c_void,
                    Option<*const std::ffi::c_void>,
                    i32,
                ) = match wh.as_raw() {
                    #[cfg(target_os = "macos")]
                    RawWindowHandle::AppKit(h) => {
                        (h.ns_view.as_ptr() as *const std::ffi::c_void, None, 0)
                    }
                    #[cfg(target_os = "windows")]
                    RawWindowHandle::Win32(h) => {
                        (h.hwnd.get() as *const std::ffi::c_void, None, 1)
                    }
                    #[cfg(target_os = "linux")]
                    RawWindowHandle::Xcb(h) => {
                        let connection = match dh.as_raw() {
                            RawDisplayHandle::Xcb(d) => {
                                d.connection.map(|p| p.as_ptr() as *const std::ffi::c_void)
                            }
                            _ => None,
                        };
                        (h.window.get() as *const std::ffi::c_void, connection, 1)
                    }
                    #[cfg(target_os = "linux")]
                    RawWindowHandle::Xlib(h) => {
                        let display = match dh.as_raw() {
                            RawDisplayHandle::Xlib(d) => {
                                d.display.map(|p| p.as_ptr() as *const std::ffi::c_void)
                            }
                            _ => None,
                        };
                        (h.window as *const std::ffi::c_void, display, 1)
                    }
                    #[cfg(target_os = "linux")]
                    RawWindowHandle::Wayland(h) => {
                        let display = match dh.as_raw() {
                            RawDisplayHandle::Wayland(d) => {
                                Some(d.display.as_ptr() as *const std::ffi::c_void)
                            }
                            _ => None,
                        };
                        (h.surface.as_ptr() as *const std::ffi::c_void, display, 2)
                    }
                    _ => return Err("unsupported platform window handle".into()),
                };

                let mpv_handle = mpv::MpvHandle::new(
                    raw_window_ptr,
                    display_ptr,
                    embed_mode,
                    app_handle.clone(),
                )
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
            let manager_for_cleanup = core.torrents.clone();
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
            commands::torrents::add_torrent,
            commands::torrents::get_torrent_info,
            commands::torrents::list_torrents,
            commands::torrents::prepare_stream,
            commands::torrents::get_stream_status,
            commands::torrents::get_torrent_piece_ranges,
            commands::torrents::stop_stream,
            commands::torrents::wipe_all_torrent_files,
            commands::torrents::pause_torrent,
            commands::torrents::resume_torrent,
            commands::torrents::remove_torrent,
            commands::torrents::get_download_dir,
            commands::torrents::get_http_port,
            commands::torrents::save_torrent_selection,
            commands::torrents::save_multiple_torrent_selections,
            commands::torrents::get_saved_selection,
            commands::torrents::get_all_torrent_selections,
            commands::torrents::remove_saved_selection,
            commands::torrents::remove_torrent_all_assignments,
            commands::torrents::get_streaming_client,
            commands::torrents::get_torrent_files,
            commands::torrents::auto_assign_torrent_files,
            commands::torrents::prepare_playback,
            commands::torrents::prepare_saved_playback,
            commands::torrents::plan_quick_play,
            commands::search::search_torrents,
            commands::search::get_tracker_preference,
            commands::search::set_tracker_preference,
            commands::search::parse_magnet_link,
            commands::search::parse_release_title,
            commands::library::record_watch_progress,
            commands::library::get_watch_history,
            commands::library::remove_watch_history_item,
            commands::library::clear_watch_history,
            commands::library::get_watch_progress,
            commands::library::set_watch_progress,
            commands::library::remove_watch_progress_entry,
            commands::library::clear_watch_progress,
            commands::library::get_resume_target,
            commands::library::get_my_list,
            commands::library::set_my_list,
            commands::library::toggle_my_list_item,
            commands::library::save_track_preference,
            commands::library::get_track_preference,
            commands::library::save_settings,
            commands::library::get_settings,
            commands::library::check_is_anime,
            commands::library::refresh_anime_list,
            commands::library::save_cache_metadata,
            commands::library::get_cache_metadata,
            commands::library::get_all_cache_metadata,
            commands::subtitles::fetch_subtitles,
            commands::subtitles::download_subtitle,
            commands::subtitles::import_subtitle_pack,
            commands::subtitles::get_subtitle_pack_for_episode,
            commands::subtitles::get_subtitle_pack_coverage,
            commands::subtitles::remove_subtitle_pack_episode,
            commands::subtitles::clear_subtitle_pack,
            commands::extensions::list_extensions,
            commands::extensions::install_extension_from_path,
            commands::extensions::install_extension_from_url,
            commands::extensions::remove_extension,
            commands::extensions::set_extension_enabled,
            commands::extensions::set_extension_field_values,
            commands::extensions::fetch_extension_subtitles,
            commands::extensions::list_debrid_files,
            commands::extensions::resolve_debrid_stream,
            check_external_player,
            open_in_external_player,
            logger::log_message,
            download_update,
            install_update,
            updater::get_platform_info,
            updater::enable_touch_id_sudo,
            updater::is_touch_id_sudo_enabled,
            open_external_url,
            load_file,
            cycle_pause,
            seek_video,
            mpv_run_command,
            mpv_set_option_string
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
