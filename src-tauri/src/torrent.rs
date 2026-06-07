use anyhow::{Context, Result};
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions, SessionPersistenceConfig, api::TorrentIdOrHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::net::SocketAddr;
use tauri::State;
use tokio::sync::RwLock;
use axum::{
    Router,
    routing::get,
    extract::Path,
    response::{IntoResponse, Response},
    http::{StatusCode, header, HeaderMap},
    body::Body,
};
use tower_http::cors::CorsLayer;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub path: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AudioTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub index: usize,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub index: usize,
    pub title: Option<String>,
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MkvMetadata {
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub handle_id: usize,
    pub name: String,
    pub size: u64,
    pub files: Vec<TorrentFile>,
    pub progress: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub peers: usize,
    pub is_paused: bool,
    pub state: String, // "checking", "downloading", "paused", "live"
}

#[derive(Clone, Serialize)]
pub struct StreamInfo {
    pub url: String,
    pub file_name: String,
    pub file_size: u64,
    pub metadata: Option<MkvMetadata>,
}

#[derive(Clone, Serialize)]
pub struct StreamStatus {
    pub status: String, // "initializing", "ready", "error"
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub peers: usize,
    pub download_speed: u64,
    pub stream_info: Option<StreamInfo>,
    pub state: String, // "checking", "downloading"
}

#[derive(Clone)]
pub struct AppState {
    pub session: Arc<Session>,
    pub download_dir: PathBuf,
}

struct TorrentEntry {
    magnet_url: String,
    session_id: Option<usize>, // None if not yet added to session
    buffer_start: Option<std::time::Instant>, // Set when buffering begins (prepare_stream done)
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedTorrent {
    handle_id: usize,
    session_id: usize,
    magnet_url: String,
    #[serde(with = "systemtime_serde")]
    cached_at: std::time::SystemTime,
}

// Serde helper for SystemTime
mod systemtime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap();
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}


pub struct TorrentManager {
    session: Arc<Session>,
    download_dir: PathBuf,
    torrents: Arc<RwLock<HashMap<usize, TorrentEntry>>>,
    next_id: Arc<RwLock<usize>>,
    http_addr: SocketAddr,
    // Torrent cache: keep up to 10 torrents paused with data cleared
    torrent_cache: Arc<RwLock<Vec<CachedTorrent>>>,
}

async fn stream_file(
    Path((session_id, file_id)): Path<(usize, usize)>,
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    use std::io::SeekFrom;
    use tokio_util::io::ReaderStream;

    let handle = match state.session.get(TorrentIdOrHash::Id(session_id)) {
        Some(h) => h,
        None => return (StatusCode::NOT_FOUND, "Torrent not found").into_response(),
    };

    let file_size = match handle.with_metadata(|meta| {
        meta.file_infos.get(file_id).map(|f| f.len)
    }) {
        Ok(Some(size)) => size,
        _ => return (StatusCode::NOT_FOUND, "File not found").into_response(),
    };

    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    
    let (start, end, status_code) = if let Some(range_str) = range {
        if let Some(range_values) = range_str.strip_prefix("bytes=") {
            let parts: Vec<&str> = range_values.split('-').collect();
            let start = parts[0].parse::<u64>().unwrap_or(0);
            let end = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse::<u64>().unwrap_or(file_size - 1).min(file_size - 1)
            } else {
                file_size - 1
            };
            (start, end, StatusCode::PARTIAL_CONTENT)
        } else {
            (0, file_size - 1, StatusCode::OK)
        }
    } else {
        (0, file_size - 1, StatusCode::OK)
    };

    let content_length = end - start + 1;

    // Pre-fetch relative filename for the disk fallback (before stream() consumes the Arc).
    let rel_path_for_fallback = handle.with_metadata(|meta| {
        meta.file_infos.get(file_id).map(|fi| fi.relative_filename.clone())
    }).ok().flatten();

    // Try the librqbit streaming path first (works for in-progress and most finished torrents).
    // If that fails (can happen on fully-finished torrents), fall back to serving the file
    // directly from disk, which always works for completed downloads.
    let body = match handle.stream(file_id) {
        Ok(mut stream) => {
            if start > 0 {
                if let Err(e) = stream.seek(SeekFrom::Start(start)).await {
                    tracing::error!("Failed to seek stream to {}: {}", start, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to seek: {}", e)).into_response();
                }
            }
            Body::from_stream(ReaderStream::new(stream.take(content_length)))
        }
        Err(stream_err) => {
            // Fall back to direct disk I/O for fully-downloaded torrents.
            let rel_path = match rel_path_for_fallback {
                Some(p) => p,
                None => {
                    tracing::error!("stream() failed and metadata unavailable: {}", stream_err);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "stream unavailable".to_string()).into_response();
                }
            };
            let file_path = state.download_dir.join(&rel_path);
            match tokio::fs::File::open(&file_path).await {
                Ok(mut f) => {
                    if start > 0 {
                        use tokio::io::AsyncSeekExt;
                        if let Err(e) = f.seek(SeekFrom::Start(start)).await {
                            tracing::error!("Failed to seek file {:?}: {}", file_path, e);
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("seek failed: {}", e)).into_response();
                        }
                    }
                    Body::from_stream(ReaderStream::new(f.take(content_length)))
                }
                Err(e) => {
                    tracing::error!("stream() failed ({}), disk fallback also failed for {:?}: {}", stream_err, file_path, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("file unavailable: {}", e)).into_response();
                }
            }
        }
    };

    let mut response = Response::builder()
        .status(status_code)
        .header(header::CONTENT_TYPE, "video/x-matroska")
        .header(header::CONTENT_LENGTH, content_length.to_string())
        .header(header::ACCEPT_RANGES, "bytes");
    
    if status_code == StatusCode::PARTIAL_CONTENT {
        let content_range = format!("bytes {}-{}/{}", start, end, file_size);
        response = response.header(header::CONTENT_RANGE, content_range);
    }

    response.body(body).unwrap().into_response()
}

impl TorrentManager {
    pub async fn new(download_dir: PathBuf) -> Result<Self> {
        println!("initializing TorrentManager with download_dir: {:?}", download_dir);
        
        if let Err(e) = std::fs::create_dir_all(&download_dir) {
            eprintln!("failed to create download directory: {}", e);
            return Err(e.into());
        }

        // Boot-time cleanup: wipe any files left over from a previous session (including
        // crash survivors where cleanup_all() never ran). Magnolia is a streaming app —
        // no torrent data should persist across restarts.
        match std::fs::read_dir(&download_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Err(e) = std::fs::remove_dir_all(&path) {
                            tracing::warn!("Boot cleanup: failed to remove dir {:?}: {}", path, e);
                        }
                    } else if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!("Boot cleanup: failed to remove file {:?}: {}", path, e);
                    }
                }
                tracing::info!("Boot cleanup: cleared stale files from {:?}", download_dir);
            }
            Err(e) => tracing::warn!("Boot cleanup: could not read download dir: {}", e),
        }

        // Create session with fastresume enabled so the "all zeros" bitfield is persisted
        // after the first initial_check. On Windows, pread_exact via seek_read returns Ok()
        // on EOF rather than an error, so the initial_check has to SHA1-hash every piece
        // individually instead of short-circuiting when a file is found empty. Fastresume
        // means only the *first* play of any given torrent incurs that cost; every
        // subsequent play (same info_hash) skips the check entirely.
        println!("creating librqbit session...");
        let session = match Session::new_with_opts(download_dir.clone(), SessionOptions {
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json { folder: None }),
            ..Default::default()
        }).await {
            Ok(s) => {
                println!("librqbit session created successfully");
                s
            }
            Err(e) => {
                eprintln!("failed to create librqbit session: {}", e);
                return Err(anyhow::anyhow!("Failed to create librqbit session: {}", e));
            }
        };

        let torrents = Arc::new(RwLock::new(HashMap::new()));
        let next_id = Arc::new(RwLock::new(0));

        // Note: We don't load existing torrents from session since we store URLs separately
        // and only add them to session when streaming starts
        tracing::info!("TorrentManager initialized");

        println!("binding HTTP server to localhost...");
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => {
                println!("HTTP server listener created successfully");
                l
            }
            Err(e) => {
                eprintln!("failed to bind HTTP server: {}", e);
                return Err(e.into());
            }
        };
        let http_addr = listener.local_addr()?;
        println!("HTTP server will run on: {}", http_addr);
        
        let state = AppState {
            session: session.clone(),
            download_dir: download_dir.clone(),
        };

        let app = Router::new()
            .route("/torrents/{session_id}/stream/{file_id}", get(stream_file))
            .route("/fonts/{filename}", get(serve_font))
            .layer(CorsLayer::permissive())
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let manager = Self {
            session,
            download_dir,
            torrents,
            next_id,
            http_addr,
            torrent_cache: Arc::new(RwLock::new(Vec::new())),
        };
        
        // Load cached torrents from disk
        if let Err(e) = manager.load_cache_from_disk().await {
            tracing::warn!("Failed to load torrent cache from disk: {}", e);
        }
        
        Ok(manager)
    }

    pub async fn add_torrent(&self, magnet_or_url: String) -> Result<usize> {
        let t0 = std::time::Instant::now();
        tracing::info!("[player-init] add_torrent: starting (list_only metadata fetch)");
        
        let add_torrent = if magnet_or_url.starts_with("magnet:") {
            AddTorrent::from_url(&magnet_or_url)
        } else if magnet_or_url.starts_with("http") {
            AddTorrent::from_url(&magnet_or_url)
        } else {
            AddTorrent::from_local_filename(&magnet_or_url)?
        };
        
        let opts = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        
        let response = self.session.add_torrent(add_torrent, Some(opts)).await?;
        
        // Extract session_id if it was added (shouldn't happen with list_only, but handle it)
        let session_id = match response {
            AddTorrentResponse::Added(id, _) | AddTorrentResponse::AlreadyManaged(id, _) => {
                tracing::info!("Torrent was added to session with id: {}", id);
                Some(id)
            }
            AddTorrentResponse::ListOnly(_) => {
                tracing::info!("Got list-only response (metadata fetched)");
                None
            }
        };
        
        let mut id_lock = self.next_id.write().await;
        let our_id = *id_lock;
        *id_lock += 1;
        drop(id_lock);
        
        let mut torrents = self.torrents.write().await;
        torrents.insert(our_id, TorrentEntry {
            magnet_url: magnet_or_url,
            session_id,
            buffer_start: None,
        });
        
        tracing::info!("[player-init] add_torrent: done, our_id={} in {:.3}s", our_id, t0.elapsed().as_secs_f64());
        Ok(our_id)
    }

    pub async fn get_torrent_info(&self, handle_id: usize) -> Result<TorrentInfo> {
        let torrents = self.torrents.read().await;
        let entry = torrents
            .get(&handle_id)
            .context("Torrent handle not found")?;
        
        // If not yet added to session, fetch metadata via list_only
        if entry.session_id.is_none() {
            let magnet_url = entry.magnet_url.clone();
            drop(torrents);
            
            let add_torrent = if magnet_url.starts_with("magnet:") {
                AddTorrent::from_url(&magnet_url)
            } else if magnet_url.starts_with("http") {
                AddTorrent::from_url(&magnet_url)
            } else {
                AddTorrent::from_local_filename(&magnet_url)?
            };
            
            let opts = AddTorrentOptions {
                list_only: true,
                ..Default::default()
            };
            
            let response = self.session.add_torrent(add_torrent, Some(opts)).await?;
            
            match response {
                AddTorrentResponse::ListOnly(list_info) => {
                    let files: Vec<TorrentFile> = list_info.info
                        .iter_file_details()?
                        .enumerate()
                        .filter_map(|(index, detail)| {
                            let filename_str = detail.filename.to_string().ok()?;
                            let lower = filename_str.to_lowercase();
                            if lower.ends_with(".mkv") || lower.ends_with(".mp4") || lower.ends_with(".avi") || lower.ends_with(".mov") {
                                let pathbuf = detail.filename.to_pathbuf().ok()?;
                                let name = pathbuf
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                
                                Some(TorrentFile {
                                    index,
                                    name,
                                    size: detail.len,
                                    path: filename_str,
                                })
                            } else {
                                None
                            }
                        })
                        .collect();
                    
                    let name = match &list_info.info.name {
                        Some(n) => n.to_string(),
                        None => "Unknown".to_string(),
                    };
                    
                    return Ok(TorrentInfo {
                        handle_id,
                        name,
                        size: files.iter().map(|f| f.size).sum(),
                        files,
                        progress: 0.0,
                        download_speed: 0,
                        upload_speed: 0,
                        peers: 0,
                        is_paused: true,
                        state: "paused".to_string(),
                    });
                }
                _ => {
                    return Err(anyhow::anyhow!("Expected list_only response"));
                }
            }
        }
        
        let session_id = entry.session_id.unwrap();

        let handle = self
            .session
            .get(TorrentIdOrHash::Id(session_id))
            .context("Session torrent not found")?;

        // Get torrent metadata - filter to video files (.mkv, .mp4, .avi, .mov)
        let files: Vec<TorrentFile> = handle
            .with_metadata(|meta| {
                meta.file_infos
                    .iter()
                    .enumerate()
                    .filter_map(|(index, file_info)| {
                        let filename = file_info
                            .relative_filename
                            .to_string_lossy()
                            .to_string();
                        let lower = filename.to_lowercase();
                        
                        if lower.ends_with(".mkv") || lower.ends_with(".mp4") || lower.ends_with(".avi") || lower.ends_with(".mov") {
                            Some(TorrentFile {
                                index,
                                name: file_info
                                    .relative_filename
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                size: file_info.len,
                                path: filename,
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            })?;

        let torrent_name = handle.name().unwrap_or_else(|| "Unknown".to_string());
        let stats = handle.stats();
        let is_paused = handle.is_paused();
        
        // Determine state: when stats.live is None, torrent is checking/hashing
        let state = if is_paused {
            "paused".to_string()
        } else if stats.live.is_none() {
            "checking".to_string()
        } else {
            "live".to_string()
        };

        Ok(TorrentInfo {
            handle_id,
            name: torrent_name,
            size: files.iter().map(|f| f.size).sum(),
            files,
            progress: if stats.total_bytes > 0 {
                stats.progress_bytes as f64 / stats.total_bytes as f64 * 100.0
            } else {
                0.0
            },
            download_speed: stats
                .live
                .as_ref()
                .map(|l| l.download_speed.mbps as u64)
                .unwrap_or(0),
            upload_speed: stats
                .live
                .as_ref()
                .map(|l| l.upload_speed.mbps as u64)
                .unwrap_or(0),
            peers: stats.live.as_ref().map(|l| l.snapshot.peer_stats.live).unwrap_or(0),
            is_paused,
            state,
        })
    }

    pub async fn list_torrents(&self) -> Result<Vec<TorrentInfo>> {
        let torrents = self.torrents.read().await;
        let mut result = Vec::new();

        for (our_id, _) in torrents.iter() {
            if let Ok(info) = self.get_torrent_info(*our_id).await {
                result.push(info);
            }
        }

        Ok(result)
    }

    pub async fn prepare_stream(&self, handle_id: usize, file_index: usize) -> Result<()> {
        let t0 = std::time::Instant::now();
        tracing::info!("[player-init] prepare_stream: start handle_id={} file_index={}", handle_id, file_index);
        let torrents = self.torrents.read().await;
        let entry = torrents
            .get(&handle_id)
            .context("Torrent handle not found")?;
        
        // Check if this torrent is in the cache
        let mut cache = self.torrent_cache.write().await;
        let cached_session_id = cache.iter()
            .find(|ct| ct.handle_id == handle_id)
            .map(|ct| ct.session_id);
        
        if let Some(session_id) = cached_session_id {
            // Remove from cache
            cache.retain(|ct| ct.handle_id != handle_id);
            drop(cache);

            // The torrent is still paused in the rqbit session (files were cleared by
            // stop_stream but the session entry is kept so the persisted bitfield survives).
            // Just update only_files to the requested file and unpause — no re-checking phase.
            if let Some(handle) = self.session.get(TorrentIdOrHash::Id(session_id)) {
                let only_files_set = std::collections::HashSet::from([file_index]);
                if let Err(e) = self.session.update_only_files(&handle, &only_files_set).await {
                    tracing::warn!("Failed to update only_files for cached session {}: {}", session_id, e);
                }

                if handle.is_paused() {
                    self.session.unpause(&handle).await?;
                }

                tracing::info!("[player-init] prepare_stream: resumed from cache session_id={} in {:.3}s", session_id, t0.elapsed().as_secs_f64());

                drop(torrents);
                let mut torrents = self.torrents.write().await;
                if let Some(entry) = torrents.get_mut(&handle_id) {
                    entry.session_id = Some(session_id);
                    entry.buffer_start = Some(std::time::Instant::now());
                }
                tracing::info!("[player-init] buffer: start (cache path)");

                return Ok(());
            } else {
                tracing::warn!("Cached session_id {} not found in rqbit session, will re-add fresh", session_id);
                // Fall through to fresh add with only_files below.
            }
        } else {
            drop(cache);
        }
        
        // Add the torrent with ONLY the specific file selected
        let add_torrent = if entry.magnet_url.starts_with("magnet:") {
            AddTorrent::from_url(&entry.magnet_url)
        } else if entry.magnet_url.starts_with("http") {
            AddTorrent::from_url(&entry.magnet_url)
        } else {
            AddTorrent::from_local_filename(&entry.magnet_url)?
        };
        
        tracing::info!("[player-init] prepare_stream: no cache hit, calling session.add_torrent (t={:.3}s)", t0.elapsed().as_secs_f64());
        
        let opts = AddTorrentOptions {
            overwrite: true,
            paused: false,
            only_files: Some(vec![file_index]),
            force_tracker_interval: Some(std::time::Duration::from_secs(5)), // Request peers faster
            ..Default::default()
        };
        
        let add_t = std::time::Instant::now();
        let response = self.session.add_torrent(add_torrent, Some(opts)).await?;
        tracing::info!("[player-init] prepare_stream: session.add_torrent returned in {:.3}s", add_t.elapsed().as_secs_f64());
        let (session_id, _handle) = match response {
            AddTorrentResponse::Added(id, h) => (id, h),
            AddTorrentResponse::AlreadyManaged(id, h) => {
                tracing::info!("[player-init] prepare_stream: torrent already managed, reusing");
                if h.is_paused() {
                    self.session.unpause(&h).await?;
                }
                (id, h)
            }
            AddTorrentResponse::ListOnly(_) => {
                return Err(anyhow::anyhow!("Unexpected list_only response"));
            }
        };
        
        tracing::info!("[player-init] prepare_stream: setting session_id={} for handle_id={}", session_id, handle_id);
        
        drop(torrents);
        let mut torrents = self.torrents.write().await;
        if let Some(entry) = torrents.get_mut(&handle_id) {
            entry.session_id = Some(session_id);
            entry.buffer_start = Some(std::time::Instant::now());
        }
        tracing::info!("[player-init] buffer: start (fresh path)");
        tracing::info!("[player-init] prepare_stream: complete in {:.3}s", t0.elapsed().as_secs_f64());
        
        Ok(())
    }

    pub async fn get_stream_status(&self, handle_id: usize, file_index: usize) -> Result<StreamStatus> {
        let torrents = self.torrents.read().await;
        let entry = torrents
            .get(&handle_id)
            .context("Torrent handle not found")?;

        let session_id = entry.session_id.context("Torrent not yet added to session")?;
        let buffer_start = entry.buffer_start;

        let handle = self.session.get(TorrentIdOrHash::Id(session_id)).context("Session torrent not found")?;
        let stats = handle.stats();

        let file_info = handle.with_metadata(|meta| {
            meta.file_infos.get(file_index).map(|fi| (
                fi.relative_filename.clone(),
                fi.len
            ))
        })?.context("File index out of range")?;

        let (file_name_path, file_size) = file_info;
        let file_name = file_name_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let is_streamable = handle.clone().stream(file_index).is_ok();
        let has_buffer = stats.progress_bytes > 2 * 1024 * 1024 || stats.finished;
        // A finished torrent can always be served via HTTP even if stream() fails.
        let is_ready = has_buffer && (is_streamable || stats.finished);

        let stream_info = if is_ready {
            Some(StreamInfo {
                url: format!(
                    "http://{}/torrents/{}/stream/{}",
                    self.http_addr,
                    session_id,
                    file_index
                ),
                file_name,
                file_size,
                metadata: None,
            })
        } else {
            None
        };

        let state = if stats.live.is_none() {
            "checking".to_string()
        } else if stats.finished {
            "ready".to_string()
        } else {
            "downloading".to_string()
        };

        let status = if is_ready { "ready".to_string() } else { "initializing".to_string() };

        let peers = stats.live.as_ref().map(|l| l.snapshot.peer_stats.live).unwrap_or(0);
        let download_mbps = stats.live.as_ref().map(|l| l.download_speed.mbps).unwrap_or_default();
        let elapsed_str = buffer_start
            .map(|t| format!("{:.1}s", t.elapsed().as_secs_f64()))
            .unwrap_or_else(|| "?".to_string());
        tracing::info!(
            "[player-init] buffer poll t={}: state={} {}/{} bytes  {} peers  {:.2}Mbps  ready={}",
            elapsed_str, state, stats.progress_bytes, stats.total_bytes, peers, download_mbps, is_ready
        );

        Ok(StreamStatus {
            status,
            progress_bytes: stats.progress_bytes,
            total_bytes: stats.total_bytes,
            peers,
            download_speed: download_mbps as u64,
            stream_info,
            state,
        })
    }

        pub async fn stop_stream(&self, handle_id: usize, delete_files: bool) -> Result<()> {
        tracing::info!("Stopping stream for handle_id: {}, delete_files: {}", handle_id, delete_files);
        
        let mut torrents = self.torrents.write().await;
        if let Some(entry) = torrents.get_mut(&handle_id) {
            if let Some(session_id) = entry.session_id {
                if delete_files {
                    // Delete torrent completely from librqbit with all files
                    tracing::info!("Deleting torrent session_id: {} completely with files", session_id);
                    
                    // First, manually delete the files to ensure they're removed
                    if let Some(handle) = self.session.get(TorrentIdOrHash::Id(session_id)) {
                        tracing::info!("Manually deleting files for session_id: {}", session_id);
                        self.clear_torrent_files(session_id, &handle).await?;
                    }
                    
                    // Then remove from librqbit
                    entry.session_id = None;
                    self.session.delete(TorrentIdOrHash::Id(session_id), true).await?;
                    tracing::info!("Torrent session_id: {} deleted from librqbit", session_id);
                } else {
                    // Cache the torrent: pause it and clear file data
                    tracing::info!("Caching torrent session_id: {} for handle_id: {}", session_id, handle_id);
                    
                    // Pause the torrent first
                    if let Some(handle) = self.session.get(TorrentIdOrHash::Id(session_id)) {
                        self.session.pause(&handle).await?;
                        
                        // Clear file data to save space
                        self.clear_torrent_files(session_id, &handle).await?;
                    }
                    
                    // Add to cache
                    let cached_torrent = CachedTorrent {
                        handle_id,
                        session_id,
                        magnet_url: entry.magnet_url.clone(),
                        cached_at: std::time::SystemTime::now(),
                    };
                    
                    let mut cache = self.torrent_cache.write().await;
                    
                    // Remove if already in cache (by handle_id or magnet_url)
                    let magnet_url = entry.magnet_url.clone();
                    cache.retain(|ct| ct.handle_id != handle_id && ct.magnet_url != magnet_url);
                    
                    // Add to front of cache
                    cache.insert(0, cached_torrent);
                    
                    // Enforce 10 torrent limit
                    while cache.len() > 10 {
                        if let Some(oldest) = cache.pop() {
                            tracing::info!("Cache limit reached, removing oldest cached torrent: handle_id={}, session_id={}", oldest.handle_id, oldest.session_id);
                            // Remove from session completely
                            self.session.delete(TorrentIdOrHash::Id(oldest.session_id), true).await?;
                            
                            // Clear session_id from torrent entry
                            if let Some(old_entry) = torrents.get_mut(&oldest.handle_id) {
                                old_entry.session_id = None;
                            }
                        }
                    }
                    
                    tracing::info!("Torrent cached. Current cache size: {}", cache.len());
                    drop(cache);
                    
                    // Save cache to disk
                    if let Err(e) = self.save_cache_to_disk().await {
                        tracing::error!("Failed to save cache to disk: {}", e);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Clear file data for a cached torrent to save space while keeping metadata
    async fn clear_torrent_files(&self, session_id: usize, handle: &librqbit::ManagedTorrent) -> Result<()> {
        tracing::info!("Clearing file data for session_id: {}", session_id);
        
        // Get torrent name for the base directory
        let torrent_name = handle.with_metadata(|meta| {
            meta.info.name.as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        })?;
        
        // Get file paths
        let file_paths: Vec<PathBuf> = handle.with_metadata(|meta| {
            meta.file_infos
                .iter()
                .map(|info| {
                    let mut path = self.download_dir.clone();
                    path.push(info.relative_filename.to_path_buf());
                    path
                })
                .collect()
        })?;
        
        // Delete each file
        let mut deleted_count = 0;
        for path in &file_paths {
            if path.exists() {
                match tokio::fs::remove_file(&path).await {
                    Ok(_) => {
                        tracing::info!("Deleted file: {:?}", path);
                        deleted_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to delete file {:?}: {}", path, e);
                    }
                }
            }
        }
        
        // Delete the torrent's base directory if it exists and is now empty
        let torrent_dir = self.download_dir.join(&torrent_name);
        if torrent_dir.exists() && torrent_dir.is_dir() {
            match tokio::fs::remove_dir_all(&torrent_dir).await {
                Ok(_) => {
                    tracing::info!("Deleted torrent directory: {:?}", torrent_dir);
                }
                Err(e) => {
                    tracing::warn!("Failed to delete torrent directory {:?}: {}", torrent_dir, e);
                }
            }
        }
        
        tracing::info!("Deleted {} files for session_id: {}", deleted_count, session_id);
        Ok(())
    }
    
    /// Save torrent cache to disk
    async fn save_cache_to_disk(&self) -> Result<()> {
        let cache = self.torrent_cache.read().await;
        let cache_file = self.download_dir.join("torrent_cache.json");
        
        let json = serde_json::to_string_pretty(&*cache)?;
        tokio::fs::write(&cache_file, json).await?;
        
        tracing::debug!("Saved {} cached torrents to disk", cache.len());
        Ok(())
    }
    
    /// Load torrent cache from disk and restore sessions
    async fn load_cache_from_disk(&self) -> Result<()> {
        let cache_file = self.download_dir.join("torrent_cache.json");
        
        if !cache_file.exists() {
            return Ok(());
        }
        
        let json = tokio::fs::read_to_string(&cache_file).await?;
        let cached_torrents: Vec<CachedTorrent> = serde_json::from_str(&json)?;
        
        tracing::info!("Loading {} cached torrents from disk", cached_torrents.len());
        
        let mut restored_cache = Vec::new();
        
        for cached in cached_torrents {
            // Restore the torrent session in paused state
            // Don't use list_only - we want the torrent in the session with 0-byte files
            let add_torrent = AddTorrent::from_url(&cached.magnet_url);
            
            let opts = AddTorrentOptions {
                overwrite: false,
                paused: true, // Start paused to avoid downloading
                only_files: None, // No specific files selected yet
                ..Default::default()
            };
            
            match self.session.add_torrent(add_torrent, Some(opts)).await {
                Ok(response) => {
                    let session_id = match response {
                        AddTorrentResponse::Added(id, _) => {
                            tracing::info!("Restored cached torrent (newly added): handle_id={}, session_id={}", cached.handle_id, id);
                            id
                        }
                        AddTorrentResponse::AlreadyManaged(id, _) => {
                            tracing::info!("Restored cached torrent (already managed): handle_id={}, session_id={}", cached.handle_id, id);
                            id
                        }
                        AddTorrentResponse::ListOnly(_) => {
                            tracing::warn!("Got ListOnly response when trying to restore cached torrent handle_id={}", cached.handle_id);
                            continue;
                        }
                    };
                    
                    // Add to restored cache regardless of session_id match
                    // (session_id might be different if librqbit reassigns IDs)
                    let mut updated_cached = cached.clone();
                    updated_cached.session_id = session_id;
                    restored_cache.push(updated_cached);
                }
                Err(e) => {
                    tracing::warn!("Failed to restore cached torrent handle_id={}: {}", cached.handle_id, e);
                }
            }
        }
        
        let mut cache = self.torrent_cache.write().await;
        *cache = restored_cache;
        
        tracing::info!("Successfully restored {} cached torrents", cache.len());
        
        // Re-save cache with updated session IDs
        drop(cache);
        if let Err(e) = self.save_cache_to_disk().await {
            tracing::error!("Failed to save updated cache: {}", e);
        }
        
        Ok(())
    }

    pub async fn pause_torrent(&self, handle_id: usize) -> Result<()> {
        let torrents = self.torrents.read().await;
        let entry = torrents.get(&handle_id).context("Torrent not found")?;
        if let Some(session_id) = entry.session_id {
            let handle = self
                .session
                .get(TorrentIdOrHash::Id(session_id))
                .context("Session torrent not found")?;
            self.session.pause(&handle).await?;
        }
        Ok(())
    }

    pub async fn resume_torrent(&self, handle_id: usize) -> Result<()> {
        let torrents = self.torrents.read().await;
        let entry = torrents.get(&handle_id).context("Torrent not found")?;
        if let Some(session_id) = entry.session_id {
            let handle = self
                .session
                .get(TorrentIdOrHash::Id(session_id))
                .context("Session torrent not found")?;
            self.session.unpause(&handle).await?;
        }
        Ok(())
    }

    pub async fn remove_torrent(&self, handle_id: usize, delete_files: bool) -> Result<()> {
        let mut torrents = self.torrents.write().await;
        if let Some(entry) = torrents.remove(&handle_id) {
            if let Some(session_id) = entry.session_id {
                self.session.delete(TorrentIdOrHash::Id(session_id), delete_files).await?;
            }
        }
        Ok(())
    }

    pub fn get_download_dir(&self) -> PathBuf {
        self.download_dir.clone()
    }

    pub async fn wipe_all_files(&self) -> Result<()> {
        let t0 = std::time::Instant::now();
        tracing::info!("[player-init] wipe_all_files: starting");
        
        let download_dir = self.download_dir.clone();
        
        // Delete everything in the download directory except cache files
        if download_dir.exists() {
            let mut entries = tokio::fs::read_dir(&download_dir).await?;
            let mut deleted_count = 0;
            
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                
                // Skip cache files
                if file_name == "torrent_cache.json" {
                    continue;
                }
                
                if path.is_dir() {
                    match tokio::fs::remove_dir_all(&path).await {
                        Ok(_) => {
                            tracing::info!("Deleted directory: {:?}", path);
                            deleted_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to delete directory {:?}: {}", path, e);
                        }
                    }
                } else {
                    match tokio::fs::remove_file(&path).await {
                        Ok(_) => {
                            tracing::info!("Deleted file: {:?}", path);
                            deleted_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to delete file {:?}: {}", path, e);
                        }
                    }
                }
            }
            
            tracing::info!("[player-init] wipe_all_files: wiped {} items in {:.3}s", deleted_count, t0.elapsed().as_secs_f64());
        } else {
            tracing::info!("[player-init] wipe_all_files: directory empty, took {:.3}s", t0.elapsed().as_secs_f64());
        }
        
        Ok(())
    }

    pub async fn cleanup_all(&self) -> Result<()> {
        tracing::info!("Cleaning up all torrents on app close");
        let torrents = self.torrents.read().await;
        let session_ids: Vec<usize> = torrents.values()
            .filter_map(|entry| entry.session_id)
            .collect();
        drop(torrents);

        for session_id in session_ids {
            tracing::info!("Deleting torrent session_id: {} with files", session_id);
            if let Err(e) = self.session.delete(TorrentIdOrHash::Id(session_id), true).await {
                tracing::error!("Error deleting torrent {}: {}", session_id, e);
            }
        }
        
        // Also delete the cache file
        let cache_path = self.download_dir.join("torrent_cache.json");
        if cache_path.exists() {
            if let Err(e) = std::fs::remove_file(&cache_path) {
                tracing::error!("Failed to remove cache file: {}", e);
            } else {
                tracing::info!("Removed torrent cache file");
            }
        }
        
        Ok(())
    }

    pub async fn get_http_port(&self) -> Result<u16, String> {
        Ok(self.http_addr.port())
    }
}

// HTTP handler to serve fonts from app data directory
async fn serve_font(
    Path(filename): Path<String>,
) -> impl IntoResponse {
    // Get fonts directory from app data
    // Note: In Axum handlers we can't easily access AppHandle, so we'll construct the path manually
    // The fonts are stored in AppData/Roaming/com.chair.magnolia/fonts/
    
    let app_data = match dirs::data_dir() {
        Some(dir) => dir.join("com.chair.magnolia").join("fonts"),
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "Could not find app data directory").into_response(),
    };
    
    let font_path = app_data.join(&filename);
    
    // Security: ensure the path is within fonts directory
    if !font_path.starts_with(&app_data) {
        return (StatusCode::FORBIDDEN, "Access denied").into_response();
    }
    
    // Read font file
    let font_data = match tokio::fs::read(&font_path).await {
        Ok(data) => data,
        Err(_) => return (StatusCode::NOT_FOUND, "Font not found").into_response(),
    };
    
    // Determine content type based on extension
    let content_type = if filename.ends_with(".ttf") {
        "font/ttf"
    } else if filename.ends_with(".otf") {
        "font/otf"
    } else if filename.ends_with(".woff") {
        "font/woff"
    } else if filename.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    };
    
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "public, max-age=31536000".parse().unwrap());
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    
    (StatusCode::OK, headers, font_data).into_response()
}

// Tauri commands
#[tauri::command]
pub async fn add_torrent(
    manager: State<'_, Arc<TorrentManager>>,
    magnet_or_url: String,
) -> Result<usize, String> {
    manager
        .add_torrent(magnet_or_url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_torrent_info(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
) -> Result<TorrentInfo, String> {
    manager
        .get_torrent_info(handle_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_torrents(
    manager: State<'_, Arc<TorrentManager>>,
) -> Result<Vec<TorrentInfo>, String> {
    manager.list_torrents().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn prepare_stream(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    file_index: usize,
) -> Result<(), String> {
    manager
        .prepare_stream(handle_id, file_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stream_status(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    file_index: usize,
) -> Result<StreamStatus, String> {
    manager
        .get_stream_status(handle_id, file_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_torrent(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
) -> Result<(), String> {
    manager
        .pause_torrent(handle_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resume_torrent(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
) -> Result<(), String> {
    manager
        .resume_torrent(handle_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_torrent(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    delete_files: bool,
) -> Result<(), String> {
    manager
        .remove_torrent(handle_id, delete_files)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_stream(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    delete_files: bool,
) -> Result<(), String> {
    manager
        .stop_stream(handle_id, delete_files)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn wipe_all_torrent_files(
    manager: State<'_, Arc<TorrentManager>>,
) -> Result<(), String> {
    manager
        .wipe_all_files()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_download_dir(manager: State<'_, Arc<TorrentManager>>) -> Result<String, String> {
    Ok(manager
        .get_download_dir()
        .to_string_lossy()
        .to_string())
}
