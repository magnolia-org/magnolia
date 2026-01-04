use anyhow::{Context, Result};
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, api::TorrentIdOrHash};
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
// use tokio::sync::Mutex;
use ffmpeg_sidecar::paths::ffmpeg_path;

const UNSUPPORTED_AUDIO_CODECS: &[&str] = &[
    "truehd", "mlp", "pcm", "dsd",
    "dts", "dca", "dts-hd", "dtshd", "dts_hd", "dtse",
    "ac3", "eac3", "ac-3", "e-ac-3", "dolby", "atmos",
    "cook", "ra", "sipr", "wma", "wmav1", "wmav2", "wmapro",
];

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
    #[serde(default)]
    pub needs_transcoding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoded_url: Option<String>,
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
    pub needs_audio_transcoding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoded_audio_url: Option<String>,
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
    pub status: String, // "initializing", "ready", "transcoding", "error"
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub peers: usize,
    pub download_speed: u64,
    pub stream_info: Option<StreamInfo>,
    pub state: String, // "checking", "downloading", "transcoding"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcode_progress: Option<f32>, // 0.0 - 100.0
}

#[derive(Clone)]
pub struct AppState {
    pub session: Arc<Session>,
    pub transcode_states: Arc<RwLock<HashMap<(usize, usize), TranscodeState>>>,
    pub metadata_cache: Arc<RwLock<HashMap<(usize, usize), MkvMetadata>>>,
    pub download_dir: PathBuf,
}

struct TorrentEntry {
    magnet_url: String,
    session_id: Option<usize>, // None if not yet added to session
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

// Transcoding state for a specific file
#[derive(Clone)]
pub struct TranscodeState {
    pub progress: f32,
    pub output_path: Option<PathBuf>,
    pub completed: bool,
    pub error: Option<String>,
}

pub struct TorrentManager {
    session: Arc<Session>,
    download_dir: PathBuf,
    torrents: Arc<RwLock<HashMap<usize, TorrentEntry>>>,
    next_id: Arc<RwLock<usize>>,
    http_addr: SocketAddr,
    // Key: (handle_id, file_index) -> TranscodeState
    transcode_states: Arc<RwLock<HashMap<(usize, usize), TranscodeState>>>,
    // Cache metadata by (session_id, file_index)
    metadata_cache: Arc<RwLock<HashMap<(usize, usize), MkvMetadata>>>,
    // Torrent cache: keep up to 10 torrents paused with data cleared
    torrent_cache: Arc<RwLock<Vec<CachedTorrent>>>,
}

async fn get_file_metadata(
    Path((session_id, file_id)): Path<(usize, usize)>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    tracing::info!("Metadata request: session_id={}, file_id={}", session_id, file_id);
    
    let handle = match state.session.get(TorrentIdOrHash::Id(session_id)) {
        Some(h) => {
            tracing::info!("Found torrent handle for session_id={}", session_id);
            h
        },
        None => {
            tracing::error!("Torrent not found for session_id={}", session_id);
            return (StatusCode::NOT_FOUND, "Torrent not found").into_response();
        },
    };

    if handle.with_metadata(|meta| meta.file_infos.get(file_id).is_none()).unwrap_or(true) {
        tracing::error!("File not found: file_id={}", file_id);
        return (StatusCode::NOT_FOUND, "File not found").into_response();
    }
    
    tracing::info!("Creating stream for file_id={}", file_id);
    
    // Check file size first
    let file_size = match handle.with_metadata(|meta| {
        meta.file_infos.get(file_id).map(|f| f.len)
    }) {
        Ok(Some(size)) => size,
        _ => {
            tracing::error!("Could not get file size");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Could not get file size").into_response();
        }
    };
    
    tracing::info!("File size: {} bytes", file_size);
    
    // For metadata extraction, we need enough data downloaded
    // Check if we have at least 100MB or the full file if smaller
    let min_required = std::cmp::min(file_size, 100 * 1024 * 1024);
    
    let mut stream = match handle.stream(file_id) {
        Ok(s) => {
            tracing::info!("Stream created successfully");
            s
        },
        Err(e) => {
            tracing::error!("Failed to create stream for file_id {}: {}", file_id, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to stream: {}", e)).into_response();
        }
    };

    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("magnolia_metadata_{}_{}.mkv", session_id, file_id));
    
    tracing::info!("Creating temp file at: {:?}", temp_file_path);
    let mut temp_file = match tokio::fs::File::create(&temp_file_path).await {
        Ok(f) => {
            tracing::info!("Temp file created successfully");
            f
        },
        Err(e) => {
            tracing::error!("Failed to create temp file: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create temp file: {}", e)).into_response();
        }
    };

    // Read up to 100MB for metadata extraction (needs more data for complete MKV headers)
    tracing::info!("Starting to read stream data (need at least {} bytes)...", min_required);
    let mut total_read = 0usize;
    let chunk_size = 1024 * 1024; // 1MB chunks
    let max_size = std::cmp::min(file_size as usize, 100 * 1024 * 1024); // Up to 100MB
    let mut buffer = vec![0u8; chunk_size];
    
    let mut consecutive_empty_reads = 0;
    let max_empty_reads = 150; // Allow up to 150 empty reads (30 seconds total with delays) for slower connections
    
    while total_read < max_size {
        let bytes_read = match stream.read(&mut buffer).await {
            Ok(0) => {
                consecutive_empty_reads += 1;
                if consecutive_empty_reads >= max_empty_reads {
                    tracing::warn!("Too many empty reads, stopping at {} bytes", total_read);
                    break;
                }
                tracing::debug!("No data available yet, waiting... (attempt {}/{})", consecutive_empty_reads, max_empty_reads);
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                continue;
            },
            Ok(n) => {
                consecutive_empty_reads = 0; // Reset counter on successful read
                tracing::debug!("Read {} bytes (total: {})", n, total_read + n);
                n
            },
            Err(e) => {
                tracing::error!("Failed to read stream at byte {}: {}", total_read, e);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to read stream: {}", e)).into_response();
            }
        };
        
        if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut temp_file, &buffer[..bytes_read]).await {
            tracing::error!("Failed to write temp file at byte {}: {}", total_read, e);
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write temp file: {}", e)).into_response();
        }
        
        total_read += bytes_read;
    }
    
    tracing::info!("Finished reading {} bytes ({}% of target), syncing file...", 
        total_read, (total_read * 100) / max_size);
    
    // Check if we have enough data
    if total_read < 10 * 1024 * 1024 {
        tracing::error!("Not enough data read for metadata extraction: {} bytes (need at least 10MB)", total_read);
        let _ = tokio::fs::remove_file(&temp_file_path).await;
        return (StatusCode::SERVICE_UNAVAILABLE, "Not enough data available yet, please wait for torrent to buffer more data").into_response();
    }
    
    // Flush and sync the file before reading with ffprobe
    if let Err(e) = temp_file.sync_all().await {
        tracing::error!("Failed to sync temp file: {}", e);
        let _ = tokio::fs::remove_file(&temp_file_path).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to sync temp file: {}", e)).into_response();
    }
    drop(temp_file); // Close the file handle
    
    tracing::info!("File synced successfully, extracting metadata with ffprobe...");

    let metadata = match extract_mkv_metadata_ffprobe(&temp_file_path).await {
        Ok(m) => {
            tracing::info!("Metadata extracted successfully: {} audio, {} subtitle, {} chapters", 
                m.audio_tracks.len(), m.subtitle_tracks.len(), m.chapters.len());
            m
        },
        Err(e) => {
            tracing::error!("Failed to extract metadata: {}", e);
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to extract metadata: {}", e)).into_response();
        }
    };

    tracing::info!("Cleaning up temp file...");
    let _ = tokio::fs::remove_file(&temp_file_path).await;
    
    // Cache the metadata for later use by get_stream_status
    {
        let mut cache = state.metadata_cache.write().await;
        cache.insert((session_id, file_id), metadata.clone());
        tracing::info!("Cached metadata for session_id={}, file_id={}", session_id, file_id);
    }
    
    tracing::info!("Returning metadata response");
    axum::Json(metadata).into_response()
}

async fn get_subtitle_track(
    Path((session_id, file_id, track_index)): Path<(usize, usize, usize)>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    use tokio::process::Command;
    
    tracing::info!("Subtitle request: session={}, file={}, track={}", session_id, file_id, track_index);
    
    let handle = match state.session.get(TorrentIdOrHash::Id(session_id)) {
        Some(h) => h,
        None => return (StatusCode::NOT_FOUND, "Torrent not found").into_response(),
    };

    let mut stream = match handle.stream(file_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create stream: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to stream: {}", e)).into_response();
        }
    };

    // Read enough data for subtitle extraction
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("magnolia_sub_{}_{}.mkv", session_id, file_id));
    
    let mut temp_file = match tokio::fs::File::create(&temp_file_path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to create temp file: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create temp file").into_response();
        }
    };

    // Read up to 500MB to ensure we get all subtitle data
    let mut total_read = 0usize;
    let chunk_size = 1024 * 1024;
    let max_size = 500 * 1024 * 1024;
    let mut buffer = vec![0u8; chunk_size];
    
    while total_read < max_size {
        match stream.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                if tokio::io::AsyncWriteExt::write_all(&mut temp_file, &buffer[..n]).await.is_err() {
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write temp file").into_response();
                }
                total_read += n;
            }
            Err(_) => break,
        }
    }
    
    temp_file.sync_all().await.ok();
    drop(temp_file);

    // Extract subtitle using ffmpeg
    let mut cmd = Command::new("ffmpeg");
    cmd.args(&[
            "-i", temp_file_path.to_str().unwrap(),
            "-map", &format!("0:s:{}", track_index),
            "-f", "ass",
            "-"
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = match cmd.output().await {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("Failed to run ffmpeg: {}", e);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to extract subtitle").into_response();
            }
        };

    let _ = tokio::fs::remove_file(&temp_file_path).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("ffmpeg subtitle extraction failed: {}", stderr);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Subtitle extraction failed").into_response();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/x-ssa")
        .body(Body::from(output.stdout))
        .unwrap()
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

    let mut stream = match handle.stream(file_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create stream for file_id {}: {}", file_id, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to stream: {}", e)).into_response();
        }
    };

    if start > 0 {
        if let Err(e) = stream.seek(SeekFrom::Start(start)).await {
            tracing::error!("Failed to seek stream to {}: {}", start, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to seek: {}", e)).into_response();
        }
    }

    let content_length = end - start + 1;
    let limited_stream = stream.take(content_length);
    
    let reader_stream = ReaderStream::new(limited_stream);
    let body = Body::from_stream(reader_stream);

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

        // Create session with default options
        println!("creating librqbit session...");
        let session = match Session::new(download_dir.clone()).await {
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
        
        let transcode_states: Arc<RwLock<HashMap<(usize, usize), TranscodeState>>> = 
            Arc::new(RwLock::new(HashMap::new()));
        let metadata_cache: Arc<RwLock<HashMap<(usize, usize), MkvMetadata>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let state = AppState {
            session: session.clone(),
            transcode_states: transcode_states.clone(),
            metadata_cache: metadata_cache.clone(),
            download_dir: download_dir.clone(),
        };

        let app = Router::new()
            .route("/torrents/{session_id}/stream/{file_id}", get(stream_file))
            .route("/torrents/{session_id}/metadata/{file_id}", get(get_file_metadata))
            .route("/torrents/{session_id}/subtitles/{file_id}/{track_index}", get(get_subtitle_track))
            .route("/torrents/{session_id}/srt-stream/{file_id}/{track_index}", get(stream_srt_subtitles))
            .route("/torrents/{session_id}/transcoded-audio-stream/{file_id}/{track_index}", get(stream_transcoded_audio))
            .route("/torrents/{session_id}/transcoded-audio-stream/{file_id}", get(stream_transcoded_audio_default))
            .route("/torrents/{session_id}/transcoded-audio/{file_id}", get(serve_transcoded_audio))
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
            transcode_states,
            metadata_cache,
            torrent_cache: Arc::new(RwLock::new(Vec::new())),
        };
        
        // Load cached torrents from disk
        if let Err(e) = manager.load_cache_from_disk().await {
            tracing::warn!("Failed to load torrent cache from disk: {}", e);
        }
        
        Ok(manager)
    }

    pub async fn add_torrent(&self, magnet_or_url: String) -> Result<usize> {
        tracing::info!("Adding torrent with list_only to fetch metadata: {}", magnet_or_url);
        
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
        });
        
        tracing::info!("Stored torrent with our_id: {}", our_id);
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
            tracing::info!("Found cached torrent for handle_id {}, resuming session_id {}", handle_id, session_id);
            
            // Remove from cache
            cache.retain(|ct| ct.handle_id != handle_id);
            drop(cache);
            
            // Resume the torrent
            if let Some(handle) = self.session.get(TorrentIdOrHash::Id(session_id)) {
                // Resume if paused
                if handle.is_paused() {
                    self.session.unpause(&handle).await?;
                }
                
                tracing::info!("Resumed cached torrent, session_id {} for handle_id {}", session_id, handle_id);
                
                // Update entry with session_id
                drop(torrents);
                let mut torrents = self.torrents.write().await;
                if let Some(entry) = torrents.get_mut(&handle_id) {
                    entry.session_id = Some(session_id);
                }
                
                return Ok(());
            } else {
                tracing::warn!("Cached session_id {} not found in session, adding fresh", session_id);
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
        
        tracing::info!("Preparing stream for file index {}", file_index);
        
        let opts = AddTorrentOptions {
            overwrite: true,
            paused: false,
            only_files: Some(vec![file_index]),
            force_tracker_interval: Some(std::time::Duration::from_secs(5)), // Request peers faster
            ..Default::default()
        };
        
        let response = self.session.add_torrent(add_torrent, Some(opts)).await?;
        let (session_id, _handle) = match response {
            AddTorrentResponse::Added(id, h) => (id, h),
            AddTorrentResponse::AlreadyManaged(id, h) => {
                tracing::info!("Torrent already managed, reusing existing download");
                if h.is_paused() {
                    self.session.unpause(&h).await?;
                }
                (id, h)
            }
            AddTorrentResponse::ListOnly(_) => {
                return Err(anyhow::anyhow!("Unexpected list_only response"));
            }
        };
        
        tracing::info!("Setting session_id {} for handle_id {}", session_id, handle_id);
        
        drop(torrents);
        let mut torrents = self.torrents.write().await;
        if let Some(entry) = torrents.get_mut(&handle_id) {
            entry.session_id = Some(session_id);
            tracing::info!("Successfully updated entry.session_id to {}", session_id);
        }
        
        Ok(())
    }

    pub async fn get_stream_status(&self, handle_id: usize, file_index: usize) -> Result<StreamStatus> {
        println!("[Transcode] get_stream_status called: handle_id={}, file_index={}", handle_id, file_index);
        
        let torrents = self.torrents.read().await;
        let entry = torrents
            .get(&handle_id)
            .context("Torrent handle not found")?;
            
        let session_id = entry.session_id.context("Torrent not yet added to session")?;
        tracing::info!("get_stream_status for handle_id={}, session_id={}, file_index={}", handle_id, session_id, file_index);
        
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

        // Check if ready
        // We need to ensure:
        // 1. The stream can be created (handle.stream succeeds)
        // 2. We have enough data for headers (at least 2MB or finished)
        let is_streamable = handle.clone().stream(file_index).is_ok();
        let has_buffer = stats.progress_bytes > 2 * 1024 * 1024 || stats.finished;
        
        let is_ready = is_streamable && has_buffer;
        
        if !is_ready {
            tracing::debug!(
                "Stream not ready: streamable={}, buffer={} ({} bytes), finished={}", 
                is_streamable, has_buffer, stats.progress_bytes, stats.finished
            );
        }
        
        // Check transcoding state
        let transcode_progress = {
            let states = self.transcode_states.read().await;
            states.get(&(session_id, file_index)).map(|s| s.progress)
        };
        
        let transcode_completed = {
            let states = self.transcode_states.read().await;
            states.get(&(session_id, file_index)).map(|s| s.completed).unwrap_or(false)
        };
        
        let stream_info = if is_ready {
             // Extract metadata for supported video formats
            let lower = file_name.to_lowercase();
            println!("[Transcode] File name: {}, stats: {}/{} bytes", file_name, stats.progress_bytes, stats.total_bytes);
            let mut metadata = if lower.ends_with(".mkv") || lower.ends_with(".mp4") || lower.ends_with(".avi") || lower.ends_with(".mov") {
                // If fully downloaded, use the actual file
                if stats.progress_bytes >= stats.total_bytes && stats.total_bytes > 0 {
                    println!("[Transcode] File fully downloaded, extracting metadata from disk");
                    let file_path = self.download_dir.join(&file_name_path);
                    println!("[Transcode] File path: {:?}", file_path);
                    println!("[Transcode] File exists: {}", file_path.exists());
                    if file_path.exists() {
                        match extract_mkv_metadata_ffprobe(&file_path).await {
                            Ok(meta) => {
                                println!("[Transcode] Successfully extracted metadata from file");
                                Some(meta)
                            },
                            Err(e) => {
                                println!("[Transcode] Failed to extract metadata: {}", e);
                                None
                            }
                        }
                    } else {
                        println!("[Transcode] File doesn't exist on disk yet, checking cache");
                        let cache = self.metadata_cache.read().await;
                        let cached = cache.get(&(session_id, file_index)).cloned();
                        println!("[Transcode] Metadata cache contains entry: {}", cached.is_some());
                        cached
                    }
                } else {
                    println!("[Transcode] File not fully downloaded, checking metadata cache");
                    // Try to get from metadata cache (populated by /metadata/ endpoint)
                    let cache = self.metadata_cache.read().await;
                    let cached = cache.get(&(session_id, file_index)).cloned();
                    println!("[Transcode] Metadata cache contains entry: {}", cached.is_some());
                    cached
                }
            } else {
                println!("[Transcode] File format not supported for metadata extraction");
                None
            };
            println!("[Transcode] Metadata result: {}", if metadata.is_some() { "Some" } else { "None" });
            
            // If transcoding is needed and not yet started, start it
            if let Some(ref mut meta) = metadata {
                println!("[Transcode] Metadata needs_audio_transcoding: {}", meta.needs_audio_transcoding);
                if meta.needs_audio_transcoding {
                    let transcode_key = (session_id, file_index);
                    let states = self.transcode_states.read().await;
                    let transcoding_started = states.contains_key(&transcode_key);
                    drop(states);
                    
                    if !transcoding_started {
                        // Mark transcoding as started immediately - no waiting for download
                        let mut states = self.transcode_states.write().await;
                        states.insert((session_id, file_index), TranscodeState {
                            progress: 0.0,
                            output_path: None,
                            completed: false,
                            error: None,
                        });
                        drop(states);
                        
                        println!("[Transcode] Transcoding ready for immediate on-demand streaming at {}", file_name);
                        
                        // Add transcoded URLs for each audio track that needs transcoding
                        for (track_idx, track) in meta.audio_tracks.iter_mut().enumerate() {
                            if track.needs_transcoding {
                                track.transcoded_url = Some(format!(
                                    "http://{}/torrents/{}/transcoded-audio-stream/{}/{}",
                                    self.http_addr,
                                    session_id,
                                    file_index,
                                    track_idx
                                ));
                                println!("[Transcode] Track {} ({}) ready for immediate piped transcoding", 
                                    track_idx, track.codec.as_deref().unwrap_or("unknown"));
                            }
                        }
                        
                        // Keep legacy field for backward compatibility (first track needing transcode)
                        if meta.audio_tracks.first().map(|t| t.needs_transcoding).unwrap_or(false) {
                            meta.transcoded_audio_url = Some(format!(
                                "http://{}/torrents/{}/transcoded-audio-stream/{}/0",
                                self.http_addr,
                                session_id,
                                file_index
                            ));
                        }
                    }
                }
            }

            Some(StreamInfo {
                url: format!(
                    "http://{}/torrents/{}/stream/{}",
                    self.http_addr,
                    session_id,
                    file_index
                ),
                file_name,
                file_size,
                metadata,
            })
        } else {
            None
        };

        // Determine the current state
        let state = if transcode_progress.is_some() && !transcode_completed {
            "transcoding".to_string()
        } else if stats.live.is_none() {
            "checking".to_string()
        } else {
            "downloading".to_string()
        };
        
        // Check if transcoding is needed from either stream_info metadata or the cache
        let needs_transcoding_from_stream = stream_info.as_ref()
            .and_then(|s| s.metadata.as_ref())
            .map(|m| m.needs_audio_transcoding)
            .unwrap_or(false);
        
        let needs_transcoding_from_cache = {
            let cache = self.metadata_cache.read().await;
            cache.get(&(session_id, file_index))
                .map(|m| m.needs_audio_transcoding)
                .unwrap_or(false)
        };
        
        let needs_audio_transcoding = needs_transcoding_from_stream || needs_transcoding_from_cache;
        
        // Check if on-demand transcoding is ready (state exists but not completed means streaming is ready)
        let transcode_streaming_ready = transcode_progress.is_some() && !transcode_completed;
        
        // Determine status
        let status = if !is_ready {
            "initializing".to_string()
        } else if needs_audio_transcoding && !transcode_streaming_ready {
            // Still waiting for minimum download before transcoding can start
            "transcoding".to_string()
        } else {
            // Ready means either no transcoding needed, or on-demand transcoding is available
            "ready".to_string()
        };
        
        tracing::debug!("Stream status: is_ready={}, needs_transcoding={}, transcode_completed={}, status={}", 
            is_ready, needs_audio_transcoding, transcode_completed, status);

        Ok(StreamStatus {
            status,
            progress_bytes: stats.progress_bytes,
            total_bytes: stats.total_bytes,
            peers: stats.live.as_ref().map(|l| l.snapshot.peer_stats.live).unwrap_or(0),
            download_speed: stats.live.as_ref().map(|l| l.download_speed.mbps as u64).unwrap_or(0),
            stream_info,
            state,
            transcode_progress,
        })
    }
    
    pub async fn stop_stream(&self, handle_id: usize, delete_files: bool) -> Result<()> {
        tracing::info!("Stopping stream for handle_id: {}, delete_files: {}", handle_id, delete_files);
        
        let mut torrents = self.torrents.write().await;
        if let Some(entry) = torrents.get_mut(&handle_id) {
            if let Some(session_id) = entry.session_id {
                if delete_files {
                    // Full removal
                    tracing::info!("Deleting torrent session_id: {} (with files)", session_id);
                    entry.session_id = None;
                    self.session.delete(TorrentIdOrHash::Id(session_id), true).await?;
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
        
        // Delete each file - librqbit will recreate them when needed
        for path in file_paths {
            if path.exists() {
                match tokio::fs::remove_file(&path).await {
                    Ok(_) => {
                        tracing::debug!("Deleted file: {:?}", path);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to delete file {:?}: {}", path, e);
                    }
                }
            }
        }
        
        tracing::info!("Files deleted for session_id: {}", session_id);
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

    pub async fn cleanup_all(&self) -> Result<()> {
        tracing::info!("Cleaning up all torrents on app close");
        let torrents = self.torrents.read().await;
        let handles: Vec<usize> = torrents.keys().copied().collect();
        drop(torrents);

        for handle_id in handles {
            if let Err(e) = self.stop_stream(handle_id, true).await {
                tracing::error!("Error stopping torrent {}: {}", handle_id, e);
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

    pub async fn get_transcoded_audio(&self, session_id: usize, file_index: usize) -> Result<Option<Vec<u8>>, String> {
        // Check if transcoding is complete and get the output path
        let output_path = {
            let states = self.transcode_states.read().await;
            if let Some(transcode_state) = states.get(&(session_id, file_index)) {
                if !transcode_state.completed {
                    return Err("Transcoding not complete".to_string());
                }
                transcode_state.output_path.clone()
            } else {
                return Err("No transcoding in progress for this file".to_string());
            }
        };

        let output_path = match output_path {
            Some(p) => p,
            None => return Err("Transcoded file path not found".to_string()),
        };

        if !output_path.exists() {
            return Err("Transcoded file not found on disk".to_string());
        }

        // Read the entire file into memory
        match tokio::fs::read(&output_path).await {
            Ok(data) => {
                tracing::info!("Loaded transcoded audio: {} bytes from {:?}", data.len(), output_path);
                Ok(Some(data))
            }
            Err(e) => Err(format!("Failed to read transcoded audio file: {}", e)),
        }
    }
}

async fn extract_mkv_metadata_ffprobe(file_path: &std::path::Path) -> Result<MkvMetadata> {
    use tokio::process::Command;
    
    tracing::info!("Extracting metadata with ffprobe: {:?}", file_path);
    
    // Check if file exists
    if !file_path.exists() {
        tracing::error!("File does not exist: {:?}", file_path);
        return Err(anyhow::anyhow!("File does not exist"));
    }
    
    let file_size = std::fs::metadata(file_path)?.len();
    tracing::info!("File size: {} bytes", file_size);
    
    let mut cmd = Command::new("ffprobe");
    cmd.args(&[
            "-v", "error",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            "-show_chapters",
            file_path.to_str().unwrap(),
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd.output()
        .await
        .context("Failed to run ffprobe command")?;
    
    tracing::info!("ffprobe exit status: {}", output.status);
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        tracing::error!("ffprobe failed with status: {}", output.status);
        tracing::error!("ffprobe stderr: {}", stderr);
        tracing::error!("ffprobe stdout: {}", stdout);
        return Err(anyhow::anyhow!("ffprobe failed: {}", stderr));
    }
    
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    tracing::debug!("ffprobe output: {}", stdout_str);
    
    let probe_data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse ffprobe JSON output")?;
    
    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    let mut chapters = Vec::new();
    
    // Extract streams
    if let Some(streams) = probe_data.get("streams").and_then(|s| s.as_array()) {
        let mut audio_index = 0;
        let mut subtitle_index = 0;
        
        for stream in streams {
            let codec_type = stream.get("codec_type").and_then(|t| t.as_str());
            
            match codec_type {
                Some("audio") => {
                    let codec_name = stream.get("codec_name").and_then(|c| c.as_str()).unwrap_or("unknown");
                    let codec_long_name = stream.get("codec_long_name").and_then(|c| c.as_str()).unwrap_or("");
                    let profile = stream.get("profile").and_then(|p| p.as_str()).unwrap_or("");
                    let language = stream.get("tags")
                        .and_then(|t| t.get("language"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("und")
                        .to_string();
                    let title = stream.get("tags")
                        .and_then(|t| t.get("title"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                    
                    // Check if this codec needs transcoding (check codec name, long name, and profile)
                    let codec_lower = codec_name.to_lowercase();
                    let long_name_lower = codec_long_name.to_lowercase();
                    let profile_lower = profile.to_lowercase();
                    
                    // Special check for AC3/EAC3 variants to ensure proper detection
                    let is_ac3_variant = codec_lower == "ac3" 
                        || codec_lower == "eac3" 
                        || codec_lower == "ac-3" 
                        || codec_lower == "e-ac-3"
                        || codec_lower.starts_with("ac3")
                        || codec_lower.starts_with("eac3")
                        || long_name_lower.contains("ac-3")
                        || long_name_lower.contains("ac3")
                        || long_name_lower.contains("e-ac-3")
                        || long_name_lower.contains("eac3")
                        || long_name_lower.contains("dolby digital");
                    
                    // Whitelist of browser-supported codecs
                    let is_known_supported = matches!(codec_lower.as_str(), 
                        "aac" | "mp3" | "opus" | "vorbis" | "mp2" | "mp1" | "flac"
                    ) && !long_name_lower.contains("truehd") 
                      && !long_name_lower.contains("dts")
                      && !long_name_lower.contains("atmos")
                      && !is_ac3_variant;
                    
                    // Check against blacklist of known unsupported codecs
                    let is_known_unsupported = is_ac3_variant || UNSUPPORTED_AUDIO_CODECS.iter().any(|unsupported| {
                        codec_lower == *unsupported 
                            || codec_lower.contains(unsupported)
                            || long_name_lower.contains(unsupported)
                            || profile_lower.contains(unsupported)
                    });
                    
                    // Transcode if explicitly unsupported OR if not in the supported whitelist
                    let needs_transcoding = is_known_unsupported || !is_known_supported;
                    
                    tracing::info!("Audio track {}: codec='{}' ({}), profile='{}', needs_transcoding={}", 
                        audio_index, codec_name, codec_long_name, profile, needs_transcoding);
                    
                    audio_tracks.push(AudioTrack {
                        index: audio_index,
                        language: Some(language),
                        codec: Some(codec_name.to_string()),
                        name: title,
                        needs_transcoding,
                        transcoded_url: None,
                    });
                    audio_index += 1;
                }
                Some("subtitle") => {
                    let codec_name = stream.get("codec_name").and_then(|c| c.as_str()).unwrap_or("unknown");
                    let language = stream.get("tags")
                        .and_then(|t| t.get("language"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("und")
                        .to_string();
                    let title = stream.get("tags")
                        .and_then(|t| t.get("title"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                    
                    subtitle_tracks.push(SubtitleTrack {
                        index: subtitle_index,
                        language: Some(language),
                        codec: Some(codec_name.to_string()),
                        name: title,
                    });
                    subtitle_index += 1;
                }
                _ => {}
            }
        }
    }
    
    // Extract chapters
    if let Some(chapters_data) = probe_data.get("chapters").and_then(|c| c.as_array()) {
        for (index, chapter) in chapters_data.iter().enumerate() {
            let start_str = chapter.get("start_time").and_then(|s| s.as_str());
            let end_str = chapter.get("end_time").and_then(|e| e.as_str());
            let title = chapter.get("tags")
                .and_then(|t| t.get("title"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            
            if let (Some(start), Some(end)) = (start_str, end_str) {
                if let (Ok(start_time), Ok(end_time)) = (start.parse::<f64>(), end.parse::<f64>()) {
                    chapters.push(Chapter {
                        index,
                        title,
                        start_time,
                        end_time,
                    });
                }
            }
        }
    }
    
    tracing::info!("Extracted {} audio tracks, {} subtitle tracks, {} chapters", 
        audio_tracks.len(), subtitle_tracks.len(), chapters.len());
    
    // Check if ANY audio track needs transcoding (check all tracks, not just first)
    let needs_audio_transcoding = audio_tracks.iter()
        .any(|t| t.needs_transcoding);
    
    // Log all audio codecs for debugging
    for track in &audio_tracks {
        tracing::info!("Audio track {}: codec={:?}, needs_transcoding={}", 
            track.index, track.codec, track.needs_transcoding);
    }
    
    let duration = probe_data.get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok());

    if needs_audio_transcoding {
        tracing::info!("Audio transcoding required - at least one track has unsupported codec");
    } else {
        tracing::info!("No audio transcoding required - all tracks have supported codecs");
    }
    
    Ok(MkvMetadata {
        audio_tracks,
        subtitle_tracks,
        chapters,
        needs_audio_transcoding,
        transcoded_audio_url: None,
        duration,
    })
}

// Transcode audio to AAC using ffmpeg-sidecar
async fn transcode_audio_track(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    audio_track_index: usize,
    transcode_states: Arc<RwLock<HashMap<(usize, usize), TranscodeState>>>,
    session_id: usize,
    file_id: usize,
) -> Result<()> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    
    println!("[Transcode] Starting audio transcoding: {:?} -> {:?} (track {})", 
        input_path, output_path, audio_track_index);
    
    // Verify input file exists
    if !input_path.exists() {
        let err = format!("Input file does not exist: {:?}", input_path);
        println!("[Transcode] ERROR: {}", err);
        return Err(anyhow::anyhow!(err));
    }
    
    let file_size = std::fs::metadata(input_path)
        .map(|m| m.len())
        .unwrap_or(0);
    println!("[Transcode] Input file size: {} MB", file_size / 1_048_576);
    
    // Get duration for progress calculation
    let duration = get_media_duration(input_path).await.unwrap_or(0.0);
    println!("[Transcode] Media duration: {} seconds", duration);
    
    // Initialize transcode state
    {
        let mut states = transcode_states.write().await;
        states.insert((session_id, file_id), TranscodeState {
            progress: 0.0,
            output_path: Some(output_path.to_path_buf()),
            completed: false,
            error: None,
        });
    }
    
    // Use ffmpeg-sidecar to get the ffmpeg path
    use tokio::process::Command;
    
    let mut cmd = Command::new(ffmpeg_path());
    
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    cmd.args(&[
        "-y",  // Overwrite output
        "-i", input_path.to_str().unwrap(),
        "-map", &format!("0:a:{}", audio_track_index), // Select specific audio track
        "-c:a", "aac",  // Transcode to AAC
        "-b:a", "192k", // Good quality
        "-progress", "pipe:1", // Output progress to stdout
        "-nostats",
        output_path.to_str().unwrap(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    
    let mut child = cmd.spawn().context("Failed to spawn ffmpeg")?;
    
    let stdout = child.stdout.take().context("Failed to get stdout")?;
    let mut reader = BufReader::new(stdout).lines();
    
    // Parse progress output
    while let Ok(Some(line)) = reader.next_line().await {
        if line.starts_with("out_time_ms=") {
            if let Ok(time_ms) = line.trim_start_matches("out_time_ms=").parse::<i64>() {
                let current_time = time_ms as f64 / 1_000_000.0;
                let progress = if duration > 0.0 {
                    ((current_time / duration) * 100.0).min(99.0)
                } else {
                    0.0
                };
                
                // Update progress
                let mut states = transcode_states.write().await;
                if let Some(state) = states.get_mut(&(session_id, file_id)) {
                    state.progress = progress as f32;
                    if progress as u32 % 10 == 0 { // Log every 10%
                        println!("[Transcode] Progress: {:.1}%", progress);
                    }
                }
            }
        }
    }
    
    // Wait for completion
    let status = child.wait().await.context("Failed to wait for ffmpeg")?;
    
    if status.success() {
        println!("[Transcode] Completed successfully!");
        let mut states = transcode_states.write().await;
        if let Some(state) = states.get_mut(&(session_id, file_id)) {
            state.progress = 100.0;
            state.completed = true;
        }
        Ok(())
    } else {
        let error_msg = "FFmpeg transcoding failed".to_string();
        println!("[Transcode] ERROR: {}", error_msg);
        let mut states = transcode_states.write().await;
        if let Some(state) = states.get_mut(&(session_id, file_id)) {
            state.error = Some(error_msg.clone());
        }
        Err(anyhow::anyhow!(error_msg))
    }
}

// Get media duration using ffprobe
async fn get_media_duration(path: &std::path::Path) -> Result<f64> {
    use tokio::process::Command;
    
    let mut cmd = Command::new("ffprobe");
    cmd.args(&[
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap(),
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd.output()
        .await
        .context("Failed to run ffprobe")?;
    
    if output.status.success() {
        let duration_str = String::from_utf8_lossy(&output.stdout);
        duration_str.trim().parse::<f64>().context("Failed to parse duration")
    } else {
        Err(anyhow::anyhow!("ffprobe failed"))
    }
}

// HTTP handler for backward compatibility - defaults to track 0
async fn stream_transcoded_audio_default(
    Path((session_id, file_id)): Path<(usize, usize)>,
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    // Forward to main handler with track_index = 0
    stream_transcoded_audio(Path((session_id, file_id, 0)), headers, axum::extract::State(state)).await
}

// HTTP handler to stream transcoded audio live (starts playing before transcoding is complete)
async fn stream_transcoded_audio(
    Path((session_id, file_id, track_index)): Path<(usize, usize, usize)>,
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    use std::process::Stdio;
    use tokio::process::Command;
    
    tracing::info!("Live transcoded audio stream request: session_id={}, file_id={}, track_index={}", session_id, file_id, track_index);
    
    // Get torrent stream to pipe directly to ffmpeg
    let torrent_stream = {
        let handle = state.session.get(TorrentIdOrHash::Id(session_id));
        if let Some(h) = handle {
            match h.stream(file_id) {
                Ok(stream) => Some(stream),
                Err(e) => {
                    tracing::error!("Failed to create torrent stream: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };
    
    if torrent_stream.is_none() {
        return (StatusCode::NOT_FOUND, "Failed to create torrent stream").into_response();
    }
    
    let mut torrent_stream = torrent_stream.unwrap();
    
    tracing::info!("Starting real-time transcode with piped torrent stream");
    
    // Start ffmpeg transcoding with piped input from torrent stream
    let mut cmd = Command::new(ffmpeg_path());
    
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    
    let audio_map = format!("0:a:{}", track_index);
    cmd.args(&[
        "-i", "pipe:0",  // Read from stdin
        "-map", &audio_map,
        "-c:a", "aac",
        "-b:a", "192k",
        "-f", "adts",
        "pipe:1",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn ffmpeg: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start transcoding: {}", e)).into_response();
        }
    };
    
    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get ffmpeg stdin").into_response(),
    };
    
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get ffmpeg output").into_response(),
    };
    
    // Spawn task to pipe torrent stream to ffmpeg stdin
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buffer = vec![0u8; 256 * 1024]; // 256KB chunks
        let mut total_piped = 0u64;
        
        tracing::info!("Starting to pipe torrent stream to ffmpeg stdin");
        
        loop {
            match torrent_stream.read(&mut buffer).await {
                Ok(0) => {
                    tracing::info!("Torrent stream EOF, piped {} MB total", total_piped / 1_048_576);
                    break;
                }
                Ok(n) => {
                    if let Err(e) = stdin.write_all(&buffer[..n]).await {
                        tracing::error!("Failed to write to ffmpeg stdin: {}", e);
                        break;
                    }
                    total_piped += n as u64;
                    if total_piped % (50 * 1024 * 1024) == 0 {
                        tracing::info!("Piped {} MB to ffmpeg", total_piped / 1_048_576);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to read from torrent stream: {}", e);
                    break;
                }
            }
        }
        
        drop(stdin);
    });
    
    // Stream the transcoded audio to the client
    let stream = tokio_util::io::ReaderStream::new(stdout);
    let body = Body::from_stream(stream);
    
    // Spawn task to wait for ffmpeg completion (non-blocking)
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/aac")
        .header(header::TRANSFER_ENCODING, "chunked")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap()
        .into_response()
}

async fn stream_srt_subtitles(
    Path((session_id, file_id, track_index)): Path<(usize, usize, usize)>,
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    use std::process::Stdio;
    use tokio::process::Command;
    
    tracing::info!("========================================");
    tracing::info!("SRT subtitle stream request received:");
    tracing::info!("  session_id={}", session_id);
    tracing::info!("  file_id={}", file_id);
    tracing::info!("  track_index={}", track_index);
    tracing::info!("========================================");
    
    // Get the file path from the torrent session
    let file_path = {
        let handle = state.session.get(TorrentIdOrHash::Id(session_id));
        if handle.is_none() {
            tracing::error!("Session {} not found in librqbit session", session_id);
            return (StatusCode::NOT_FOUND, "Session not found").into_response();
        }
        let h = handle.unwrap();
        
        let file_info = h.with_metadata(|meta| {
            meta.file_infos.get(file_id).map(|fi| (
                fi.relative_filename.clone(),
                fi.len
            ))
        });
        
        match file_info {
            Ok(Some((file_name_path, _))) => state.download_dir.join(&file_name_path),
            _ => return (StatusCode::NOT_FOUND, "File not found in torrent").into_response(),
        }
    };
    
    if !file_path.exists() {
        tracing::warn!("File not yet available for subtitle extraction: {:?}", file_path);
        return (StatusCode::SERVICE_UNAVAILABLE, "File not yet downloaded").into_response();
    }
    
    // Parse time window from headers
    let start_time = headers.get("X-Subtitle-Start")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    
    let end_time = headers.get("X-Subtitle-End")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(start_time + 60.0);
    
    tracing::info!("Extracting SRT subtitles from {}s to {}s", start_time, end_time);
    
    // Extract subtitle track using ffmpeg
    let mut cmd = Command::new(ffmpeg_path());
    
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    cmd.args(&[
        "-ss", &start_time.to_string(),
        "-t", &(end_time - start_time).to_string(),
        "-i", file_path.to_str().unwrap(),
        "-map", &format!("0:s:{}", track_index),
        "-f", "srt",
        "pipe:1",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn ffmpeg for SRT extraction: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to extract subtitles: {}", e)).into_response();
        }
    };
    
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get ffmpeg output").into_response(),
    };
    
    // Stream the extracted subtitles to the client
    let stream = tokio_util::io::ReaderStream::new(stdout);
    let body = Body::from_stream(stream);
    
    // Spawn task to wait for ffmpeg completion (non-blocking)
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::TRANSFER_ENCODING, "chunked")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap()
        .into_response()
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

// HTTP handler to serve transcoded audio file
async fn serve_transcoded_audio(
    Path((session_id, file_id)): Path<(usize, usize)>,
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    tracing::info!("Transcoded audio request: session_id={}, file_id={}", session_id, file_id);
    
    // Check if transcoding is complete
    let output_path = {
        let states = state.transcode_states.read().await;
        if let Some(transcode_state) = states.get(&(session_id, file_id)) {
            if !transcode_state.completed {
                return (StatusCode::SERVICE_UNAVAILABLE, "Transcoding not complete").into_response();
            }
            transcode_state.output_path.clone()
        } else {
            return (StatusCode::NOT_FOUND, "No transcoding in progress").into_response();
        }
    };
    
    let output_path = match output_path {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "Transcoded file path not found").into_response(),
    };
    
    if !output_path.exists() {
        return (StatusCode::NOT_FOUND, "Transcoded file not found").into_response();
    }
    
    // Get file size
    let file_size = match tokio::fs::metadata(&output_path).await {
        Ok(m) => m.len(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get file size").into_response(),
    };
    
    // Handle range requests
    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    
    let (start, end) = if let Some(range) = range_header {
        if let Some(bytes_range) = range.strip_prefix("bytes=") {
            let parts: Vec<&str> = bytes_range.split('-').collect();
            let start: u64 = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
            let end: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(file_size - 1);
            (start, end.min(file_size - 1))
        } else {
            (0, file_size - 1)
        }
    } else {
        (0, file_size - 1)
    };
    
    let content_length = end - start + 1;
    
    // Open file and seek
    let mut file = match tokio::fs::File::open(&output_path).await {
        Ok(f) => f,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response(),
    };
    
    if start > 0 {
        if let Err(_) = file.seek(std::io::SeekFrom::Start(start)).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to seek").into_response();
        }
    }
    
    let stream = tokio_util::io::ReaderStream::new(file.take(content_length));
    let body = Body::from_stream(stream);
    
    let status = if range_header.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "audio/aac")
        .header(header::CONTENT_LENGTH, content_length.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, file_size))
        .body(body)
        .unwrap()
        .into_response()
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
pub async fn get_download_dir(manager: State<'_, Arc<TorrentManager>>) -> Result<String, String> {
    Ok(manager
        .get_download_dir()
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
pub async fn extract_subtitle(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    file_index: usize,
    track_index: usize,
) -> Result<String, String> {
    use tokio::process::Command;
    use tokio::io::AsyncReadExt;
    
    tracing::info!("Extracting subtitle chunk: handle_id={}, file_index={}, track_index={}", handle_id, file_index, track_index);
    
    // Get the torrent entry with retry for session_id
    let session_id = {
        let mut retries = 0;
        loop {
            let torrents = manager.torrents.read().await;
            let entry = torrents
                .get(&handle_id)
                .ok_or("Torrent handle not found")?;
            
            if let Some(sid) = entry.session_id {
                drop(torrents);
                break sid;
            }
            
            drop(torrents);
            
            if retries >= 10 {
                return Err("Torrent session not ready after 5 seconds. Please wait for video to start loading.".to_string());
            }
            
            retries += 1;
            tracing::info!("Session ID not yet available, waiting... (attempt {}/10)", retries);
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    };
    
    // Get handle from librqbit session
    let handle = manager.session.get(TorrentIdOrHash::Id(session_id))
        .ok_or("Session not found")?;
    
    // Get file info and size
    let (file_info, file_size) = handle.with_metadata(|meta| {
        meta.file_infos.get(file_index).map(|fi| (fi.relative_filename.clone(), fi.len))
    }).map_err(|e| e.to_string())?
        .ok_or("File index out of range")?;
    
    let file_path = manager.download_dir.join(&file_info);
    
    // Check if file is fully downloaded on disk
    if file_path.exists() {
        let metadata = tokio::fs::metadata(&file_path).await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;
        
        if metadata.len() == file_size {
            tracing::info!("File fully downloaded, extracting complete subtitles from disk");
            
            // Extract subtitle using ffmpeg directly from file
            let mut cmd = Command::new(ffmpeg_path());
            cmd.args(&[
                    "-i", file_path.to_str().unwrap(),
                    "-map", &format!("0:s:{}", track_index),
                    "-f", "srt",
                    "-"
                ]);

            #[cfg(target_os = "windows")]
            cmd.creation_flags(0x08000000);

            let output = cmd.output()
                .await
                .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!("ffmpeg subtitle extraction failed: {}", stderr);
                return Err(format!("Subtitle extraction failed: {}", stderr));
            }
            
            let subtitle_text = String::from_utf8(output.stdout)
                .map_err(|e| format!("Invalid UTF-8 in subtitle data: {}", e))?;
            
            tracing::info!("Successfully extracted complete subtitle, {} bytes", subtitle_text.len());
            return Ok(subtitle_text);
        }
    }
    
    // File not fully downloaded, extract from first 150MB for initial subtitles
    // (enough to get subtitles from beginning and throughout most movies)
    tracing::info!("Extracting initial subtitle chunk from streaming data");
    
    // Create a torrent reader for streaming
    let mut reader = handle.stream(file_index)
        .map_err(|e| format!("Failed to create stream: {}", e))?;
    
    // Read to a temporary location
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("magnolia_sub_{}_{}.mkv", session_id, file_index));
    
    let mut temp_file = tokio::fs::File::create(&temp_file_path).await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    
    // Read first 150MB or until EOF (whichever comes first)
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB chunks
    let mut total_read = 0u64;
    let initial_read_limit = 150 * 1024 * 1024; // 150MB
    
    tracing::info!("Reading initial {} MB for subtitle extraction", initial_read_limit / (1024 * 1024));
    
    while total_read < initial_read_limit {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                tracing::info!("Reached EOF at {} bytes", total_read);
                break; // EOF
            }
            Ok(n) => {
                tokio::io::AsyncWriteExt::write_all(&mut temp_file, &buffer[..n]).await
                    .map_err(|e| format!("Failed to write temp file: {}", e))?;
                total_read += n as u64;
            }
            Err(e) => {
                tracing::warn!("Error reading stream for subtitles: {}, have {} bytes", e, total_read);
                break;
            }
        }
    }
    
    temp_file.sync_all().await.map_err(|e| format!("Failed to sync temp file: {}", e))?;
    drop(temp_file);
    
    tracing::info!("Finished reading {} bytes, extracting subtitles with ffmpeg", total_read);
    
    // Extract subtitle using ffmpeg
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(&[
            "-i", temp_file_path.to_str().unwrap(),
            "-map", &format!("0:s:{}", track_index),
            "-f", "srt",
            "-"
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd.output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;
    
    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_file_path).await;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("ffmpeg subtitle extraction failed: {}", stderr);
        return Err(format!("Subtitle extraction failed: {}", stderr));
    }
    
    let subtitle_text = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in subtitle data: {}", e))?;
    
    tracing::info!("Successfully extracted initial subtitle chunk, {} bytes", subtitle_text.len());
    
    // Start background task to extract complete subtitles
    let manager_clone = manager.inner().clone();
    let handle_clone = handle_id;
    let file_clone = file_index;
    let track_clone = track_index;
    
    tokio::spawn(async move {
        if let Err(e) = extract_complete_subtitle_background(
            manager_clone,
            handle_clone,
            file_clone,
            track_clone
        ).await {
            tracing::error!("Background subtitle extraction failed: {}", e);
        }
    });
    
    Ok(subtitle_text)
}

async fn extract_complete_subtitle_background(
    manager: Arc<TorrentManager>,
    handle_id: usize,
    file_index: usize,
    track_index: usize,
) -> Result<(), String> {
    use tokio::process::Command;
    use tokio::io::AsyncReadExt;
    
    tracing::info!("[Background] Starting complete subtitle extraction for handle={}, file={}, track={}", 
        handle_id, file_index, track_index);
    
    // Small delay to let initial playback start
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Get the torrent entry
    let torrents = manager.torrents.read().await;
    let entry = torrents
        .get(&handle_id)
        .ok_or("Torrent handle not found")?;
        
    let session_id = entry.session_id.ok_or("Torrent not yet added to session")?;
    drop(torrents);
    
    // Get handle from librqbit session
    let handle = manager.session.get(TorrentIdOrHash::Id(session_id))
        .ok_or("Session not found")?;
    
    // Get file info and size
    let (file_info, file_size) = handle.with_metadata(|meta| {
        meta.file_infos.get(file_index).map(|fi| (fi.relative_filename.clone(), fi.len))
    }).map_err(|e| e.to_string())?
        .ok_or("File index out of range")?;
    
    let file_path = manager.download_dir.join(&file_info);
    
    // Check if already on disk
    if file_path.exists() {
        let metadata = tokio::fs::metadata(&file_path).await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;
        
        if metadata.len() == file_size {
            tracing::info!("[Background] File already complete on disk, no need to extract");
            return Ok(());
        }
    }
    
    // Create a torrent reader for streaming
    let mut reader = handle.stream(file_index)
        .map_err(|e| format!("Failed to create stream: {}", e))?;
    
    // Read entire file to temp location
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("magnolia_sub_full_{}_{}.mkv", session_id, file_index));
    
    let mut temp_file = tokio::fs::File::create(&temp_file_path).await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total_read = 0u64;
    
    tracing::info!("[Background] Reading complete file ({} bytes) for full subtitle extraction", file_size);
    
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                tracing::info!("[Background] Reached EOF at {} bytes", total_read);
                break;
            }
            Ok(n) => {
                tokio::io::AsyncWriteExt::write_all(&mut temp_file, &buffer[..n]).await
                    .map_err(|e| format!("Failed to write temp file: {}", e))?;
                total_read += n as u64;
                
                // Log progress every 100MB
                if total_read % (100 * 1024 * 1024) < (n as u64) {
                    tracing::info!("[Background] Progress: {} / {} bytes ({:.1}%)", 
                        total_read, file_size, (total_read as f64 / file_size as f64) * 100.0);
                }
            }
            Err(e) => {
                tracing::error!("[Background] Error reading stream: {}", e);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return Err(format!("Failed to read stream: {}", e));
            }
        }
    }
    
    temp_file.sync_all().await.map_err(|e| format!("Failed to sync: {}", e))?;
    drop(temp_file);
    
    tracing::info!("[Background] Extracting complete subtitles with ffmpeg");
    
    // Extract complete subtitle
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(&[
            "-i", temp_file_path.to_str().unwrap(),
            "-map", &format!("0:s:{}", track_index),
            "-f", "srt",
            "-"
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd.output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;
    
    let _ = tokio::fs::remove_file(&temp_file_path).await;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("[Background] ffmpeg extraction failed: {}", stderr);
        return Err(format!("Extraction failed: {}", stderr));
    }
    
    let subtitle_text = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8: {}", e))?;
    
    tracing::info!("[Background] Complete subtitle extracted, {} bytes. Saving to cache...", subtitle_text.len());
    
    // Save to cache
    // Use the magnet hash as cache key
    let torrents = manager.torrents.read().await;
    if let Some(entry) = torrents.get(&handle_id) {
        // Extract info hash from magnet link for stable cache ID
        if let Some(hash_start) = entry.magnet_url.find("btih:") {
            let hash_part = &entry.magnet_url[hash_start + 5..];
            let hash_end = hash_part.find('&').unwrap_or(hash_part.len());
            let info_hash = &hash_part[..hash_end];
            
            let cache_key = format!("{}-{}-{}", info_hash, file_index, track_index);
            
            // Save using the subtitle cache system
            if let Err(e) = tokio::fs::write(
                std::env::temp_dir().join(format!("magnolia_subtitle_cache_{}.srt", cache_key)),
                &subtitle_text
            ).await {
                tracing::error!("[Background] Failed to save subtitle cache: {}", e);
            } else {
                tracing::info!("[Background] Complete subtitle saved to cache");
            }
        }
    }
    
    Ok(())
}

#[tauri::command]
pub async fn extract_audio_track(
    manager: State<'_, Arc<TorrentManager>>,
    handle_id: usize,
    file_index: usize,
    track_index: usize,
) -> Result<Vec<u8>, String> {
    tracing::info!("extracting audio track: handle_id={}, file_index={}, track_index={}", handle_id, file_index, track_index);
    
    // Get the torrent entry with retry for session_id
    let session_id = {
        let mut retries = 0;
        loop {
            let torrents = manager.torrents.read().await;
            let entry = torrents
                .get(&handle_id)
                .ok_or("Torrent handle not found")?;
            
            if let Some(sid) = entry.session_id {
                drop(torrents);
                break sid;
            }
            
            drop(torrents);
            
            if retries >= 10 {
                return Err("Torrent session not ready after 5 seconds".to_string());
            }
            
            retries += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    };
    
    // Get handle from librqbit session
    let handle = manager.session.get(TorrentIdOrHash::Id(session_id))
        .ok_or("Session not found")?;
    
    // Get file info
    let (file_info, file_size) = handle.with_metadata(|meta| {
        meta.file_infos.get(file_index).map(|fi| (fi.relative_filename.clone(), fi.len))
    }).map_err(|e| e.to_string())?
        .ok_or("File index out of range")?;
    
    let file_path = manager.download_dir.join(&file_info);
    
    // Check if file is fully downloaded - if so, extract from disk
    if file_path.exists() {
        let metadata = tokio::fs::metadata(&file_path).await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;
        
        if metadata.len() == file_size {
            tracing::info!("File fully downloaded, extracting audio from disk");
            return extract_audio_from_file(&file_path, track_index).await;
        }
    }
    
    // File not fully downloaded, stream it to temp location first
    tracing::info!("File not fully downloaded, streaming to temp location");
    
    let mut reader = handle.stream(file_index)
        .map_err(|e| format!("Failed to create stream: {}", e))?;
    
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("magnolia_audio_{}_{}.tmp", session_id, file_index));
    
    let mut temp_file = tokio::fs::File::create(&temp_file_path).await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    
    // Copy entire file to temp location
    tracing::info!("Streaming file to temp location...");
    tokio::io::copy(&mut reader, &mut temp_file).await
        .map_err(|e| format!("Failed to stream file: {}", e))?;
    
    temp_file.sync_all().await
        .map_err(|e| format!("Failed to sync temp file: {}", e))?;
    drop(temp_file);
    
    // Extract audio from temp file
    let result = extract_audio_from_file(&temp_file_path, track_index).await;
    
    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_file_path).await;
    
    result
}

async fn extract_audio_from_file(file_path: &std::path::Path, track_index: usize) -> Result<Vec<u8>, String> {
    use tokio::process::Command;
    
    tracing::info!("Extracting audio track {} from {:?}", track_index, file_path);
    
    // Use ffmpeg to extract and remux audio track
    // Output to matroska format which can contain any codec properly
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(&[
            "-i", file_path.to_str().unwrap(),
            "-map", &format!("0:a:{}", track_index),
            "-c:a", "copy", // Copy codec without re-encoding
            "-f", "matroska", // Output as MKV which supports all codecs
            "-"
        ]);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd.output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("ffmpeg audio extraction failed: {}", stderr);
        return Err(format!("Audio extraction failed: {}", stderr));
    }
    
    tracing::info!("Successfully extracted audio track, {} bytes", output.stdout.len());
    Ok(output.stdout)
}
