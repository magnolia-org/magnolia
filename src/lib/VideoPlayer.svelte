<script>
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { MKVDemuxer } from "./mkvDemuxer.js";
  import { SubtitleRenderer } from "./subtitleRenderer.js";
  import { SRTSubtitleRenderer } from "./srtSubtitleRenderer.js";
  import { StreamingSrtFetcher } from "./streamingSrtFetcher.js";
  import { AudioPlayer } from "./audioPlayer.js";
  import { formatTime } from "./utils/timeUtils.js";
  import { fetchSubtitles, downloadSubtitle } from "./wyzieSubs.js";
  import { watchProgressStore } from "./stores/watchProgressStore.js";
  import { watchHistoryStore } from "./stores/watchHistoryStore.js";

  import { createEventDispatcher } from "svelte";

  export let src = "";
  export let metadata = null; // TMDB metadata for watch history
  export let title = "";
  export let handleId = null;
  export let fileIndex = null;
  export let magnetLink = null;
  export let initialTimestamp = 0;
  
  // Track when we should reset state (only on actual new sources, not internal src updates)
  let lastInitializedSrc = null;
  
  export let mediaId = null;
  export let mediaType = null;
  export let seasonNum = null;
  export let episodeNum = null;
  
  let videoMetadata = null; // Video file metadata (tracks, chapters, etc.)

  let loading = true;
  let loadingPhase = "initializing"; // "initializing" | "buffering" | "metadata" | "transcoding" | "demuxing" | "ready"
  let loadingStatus = {
    progress: 0,
    total: 0,
    speed: 0,
    peers: 0,
    status: "Initializing stream...",
    state: "checking",
    phaseProgress: 0, // 0-100 for current phase
  };
  let pollInterval;
  let needsAudioTranscoding = false; // Track if audio transcoding is required
  let metadataFetched = false; // Track if we've already fetched metadata during polling

  const dispatch = createEventDispatcher();

  const SEEK_TIME_SHORT = 5;
  const SEEK_TIME_LONG = 10;
  const VOLUME_STEP_SMALL = 0.1;
  const VOLUME_STEP_LARGE = 0.2;
  const CONTROLS_HIDE_TIMEOUT = 2000;
  const REFRESH_INTERVAL = 2000;

  let videoElement;
  let subtitleCanvas;
  let playing = false;
  let currentTime = 0;
  let duration = 0;
  let buffered = 0;
  let torrentBufferRanges = [];
  let volume = 1;
  let muted = false;
  let fullscreen = false;
  let wasMaximizedBeforeFullscreen = false;
  let showControls = true;
  let isBufferingSeek = false;
  let prefetchedAudio = null;
  let waitingForAudio = false;
  let justSeeked = false;
  let showBufferingIndicator = false;
  let controlsTimeout;
  let isDragging = false;
  let progressBar;
  let videoContainer;

  let demuxer = null;
  let subtitleRenderer = null;
  let srtRenderer = null;
  let audioPlayer = null;
  let useMkvDemuxer = false;
  let demuxerInitialized = false;
  let extractedFonts = [];

  let showAudioMenu = false;
  let showSubtitleMenu = false;
  let showChaptersMenu = false;
  let showPlayerMenu = false;
  let showAudioSubmenu = false;
  let showSubtitleSubmenu = false;
  let playerMenuElement = null;
  let audioSubmenuElement = null;
  let subtitleSubmenuElement = null;
  let audioSubmenuX = 0;
  let audioSubmenuY = 0;
  let subtitleSubmenuX = 0;
  let subtitleSubmenuY = 0;
  let selectedAudioTrack = 0;
  let audioTrackSwitchingSupported = false;
  let selectedSubtitleTrack = -1;
  let chapters = [];
  let externalSubtitles = [];
  let loadingExternalSubs = false;
  let loadingSubtitle = false;
  let loadingAudio = false;
  let lastSubtitleFetchKey = null;
  
  let playingInExternal = false;
  let showSkipPrompts = true;
  
  let subtitleCache = {};
  let audioCache = {};
  let streamingSrtFetcher = null;
  
  let torrentSessionId = null;
  let torrentFileId = null;
  let torrentHttpPort = null;
  let watchHistoryAdded = false;
  
  // Extract info hash from magnet link for stable caching
  function getStableCacheId() {
    if (magnetLink) {
      // Extract info hash from magnet link (xt=urn:btih:HASH)
      const match = magnetLink.match(/xt=urn:btih:([a-fA-F0-9]+)/);
      if (match && match[1]) {
        return match[1].toLowerCase();
      }
    }
    // Fallback to handleId if magnet link not available
    return handleId ? String(handleId) : '0';
  }
  
  // Load and apply track preferences for current torrent
  async function loadTrackPreferences() {
    if (!magnetLink) return;
    
    try {
      const prefs = await invoke('get_track_preference', { magnetLink });
      if (!prefs) return;
      
      console.log('[track prefs] loaded preferences:', prefs);
      
      // Apply audio track preference
      if (prefs.audio_track_index !== null && prefs.audio_track_index !== undefined) {
        const audioIndex = prefs.audio_track_index;
        if (videoMetadata?.audio_tracks?.[audioIndex]) {
          console.log(`[track prefs] auto-selecting audio track ${audioIndex}`);
          await selectAudioTrack(audioIndex);
        }
      }
      
      // Apply subtitle preference
      if (prefs.subtitle_track_index !== null && prefs.subtitle_track_index !== undefined) {
        const subIndex = prefs.subtitle_track_index;
        
        // Check if it's an external subtitle (negative index for Wyzie)
        if (subIndex < 0) {
          // Wait for external subtitles to load
          if (externalSubtitles.length > 0 && prefs.subtitle_language) {
            const matchingSub = externalSubtitles.find(sub => 
              sub.language === prefs.subtitle_language
            );
            if (matchingSub) {
              const trackIndex = externalSubtitles.indexOf(matchingSub);
              console.log(`[track prefs] auto-selecting external subtitle: ${matchingSub.language}`);
              await selectSubtitle(matchingSub, trackIndex);
            }
          }
        } else {
          // Embedded subtitle
          if (videoMetadata?.subtitle_tracks?.[subIndex]) {
            const track = videoMetadata.subtitle_tracks[subIndex];
            console.log(`[track prefs] auto-selecting embedded subtitle track ${subIndex}`);
            await selectSubtitle(track, subIndex);
          }
        }
      }
    } catch (error) {
      console.error('[track prefs] error loading preferences:', error);
    }
  }
  
  // Save current track selection as preference
  async function saveTrackPreferences() {
    if (!magnetLink) return;
    
    try {
      let subtitleLanguage = null;
      let subtitleIndex = selectedSubtitleTrack;
      
      // Determine if selected subtitle is external (Wyzie)
      if (selectedSubtitleTrack >= 0) {
        const totalEmbedded = videoMetadata?.subtitle_tracks?.length || 0;
        if (selectedSubtitleTrack >= totalEmbedded) {
          // It's an external subtitle
          const externalIndex = selectedSubtitleTrack - totalEmbedded;
          if (externalSubtitles[externalIndex]) {
            subtitleLanguage = externalSubtitles[externalIndex].language;
            subtitleIndex = -1; // Mark as external
          }
        }
      }
      
      await invoke('save_track_preference', {
        magnetLink,
        audioTrackIndex: selectedAudioTrack > 0 ? selectedAudioTrack : null,
        subtitleTrackIndex: subtitleIndex >= 0 ? subtitleIndex : null,
        subtitleLanguage
      });
      
      console.log('[track prefs] saved preferences:', {
        audio: selectedAudioTrack,
        subtitle: subtitleIndex,
        language: subtitleLanguage
      });
    } catch (error) {
      console.error('[track prefs] error saving preferences:', error);
    }
  }
  
  // Load subtitle from Tauri filesystem cache
  async function loadCachedSubtitle(cacheId, fileIndex, trackIndex) {
    try {
      const result = await invoke('load_subtitle_cache', { 
        cacheId: cacheId, 
        fileIndex: Number(fileIndex), 
        trackIndex: Number(trackIndex) 
      });
      if (result) {
        console.log(`[subtitle cache] loaded ${result.length} bytes from filesystem`);
        return result;
      }
      return null;
    } catch (error) {
      console.error('[subtitle cache] failed to load from filesystem:', error);
      return null;
    }
  }
  
  // Save subtitle to Tauri filesystem cache
  async function saveCachedSubtitle(cacheId, fileIndex, trackIndex, data) {
    try {
      await invoke('save_subtitle_cache', { 
        cacheId: cacheId, 
        fileIndex: Number(fileIndex), 
        trackIndex: Number(trackIndex),
        data: data
      });
    } catch (error) {
      console.error('[subtitle cache] failed to save to filesystem:', error);
    }
  }

  // Background task to extract complete subtitle file for caching
  async function extractCompleteSubtitleInBackground(cacheId, fileIndex, trackIndex) {
    console.log('[subtitle background] starting complete extraction for caching');
    
    // Wait a bit to let initial playback start
    await new Promise(resolve => setTimeout(resolve, 2000));
    
    try {
      const subtitleData = await invoke('extract_subtitle', {
        handleId: handleId,
        fileIndex: fileIndex,
        trackIndex: trackIndex
      });
      
      // Save to cache
      console.log('[subtitle background] complete extraction finished, saving to cache');
      await saveCachedSubtitle(cacheId, fileIndex, trackIndex, subtitleData);
      console.log('[subtitle background] complete subtitles cached successfully');
    } catch (error) {
      console.error('[subtitle background] failed to extract complete subtitles:', error);
    }
  }

  // Load audio from Tauri filesystem cache
  async function loadCachedAudio(cacheId, fileIndex, trackIndex) {
    try {
      const base64Data = await invoke('load_audio_cache', { 
        cacheId: cacheId, 
        fileIndex: Number(fileIndex), 
        trackIndex: Number(trackIndex) 
      });
      if (base64Data) {
        const audioBuffer = base64ToUint8Array(base64Data);
        console.log(`[audio cache] loaded ${audioBuffer.length} bytes from filesystem`);
        return audioBuffer;
      }
      return null;
    } catch (error) {
      console.error('[audio cache] failed to load from filesystem:', error);
      return null;
    }
  }
  
  // Helper to convert Uint8Array to base64
  function uint8ArrayToBase64(uint8Array) {
    let binary = '';
    const chunkSize = 8192;
    for (let i = 0; i < uint8Array.length; i += chunkSize) {
      const chunk = uint8Array.subarray(i, Math.min(i + chunkSize, uint8Array.length));
      binary += String.fromCharCode.apply(null, chunk);
    }
    return btoa(binary);
  }
  
  // Helper to convert base64 to Uint8Array
  function base64ToUint8Array(base64) {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
  
  // Save audio to Tauri filesystem cache
  async function saveCachedAudio(cacheId, fileIndex, trackIndex, audioBuffer) {
    try {
      // Convert to base64 for efficient transfer (Array.from is too slow for large files)
      const base64Data = uint8ArrayToBase64(audioBuffer);
      await invoke('save_audio_cache', { 
        cacheId: cacheId, 
        fileIndex: Number(fileIndex), 
        trackIndex: Number(trackIndex),
        data: base64Data
      });
      console.log(`[audio cache] saved ${audioBuffer.length} bytes to filesystem`);
    } catch (error) {
      console.error('[audio cache] failed to save to filesystem:', error);
    }
  }

  // Smooth seeking
  let isSeeking = false;
  let seekPreviewTime = 0;
  let seekTimeout;
  let hoverTime = null;
  let hoverX = 0;

  // Visual indicators for shortcuts
  let showIndicator = false;
  let indicatorType = ""; // 'seek-forward', 'seek-backward', 'volume', 'play', 'pause', 'fullscreen', 'mute', 'unmute'
  let indicatorValue = "";
  let indicatorIcon = "";
  let indicatorTimeout;
  let indicatorAnimationKey = 0;
  let indicatorNudgeKey = 0; // Separate key for nudge animations
  let seekAccumulator = 0; // Accumulates seek time for stacking
  let lastSeekDirection = null; // 'forward' or 'backward'
  let lastIndicatorType = null; // Track last indicator for nudge detection
  let progressTrackingInterval = null;
  let hasAddedToHistory = false;
  let hasSeekedToInitial = false;
  let skipSectionCheckInterval = null;

  // Skip section functionality
  const skipFilters = ['intro', 'op', 'opening', 'recap', 're-cap', 'eyecatch'];
  let currentSkipSection = null;
  let showSkipButton = false;
  let skipButtonTimeout = null;
  let skipTimeRemaining = 8;
  let skipTimerInterval = null;
  let skipTimerActive = false; // True during 8-second countdown
  let skipAnimationKey = 0; // Force animation restart
  let showNextEpisodeButton = false;

  $: if (videoElement && !useMkvDemuxer) {
    videoElement.volume = volume;
  }
  
  // Sync audio player volume when it exists (for extracted/transcoded audio tracks)
  $: if (audioPlayer && audioPlayer instanceof Audio && (selectedAudioTrack > 0 || needsAudioTranscoding)) {
    audioPlayer.volume = volume;
    audioPlayer.muted = muted;
  }
  
  // Mute video element when using extracted audio (track > 0) or transcoded audio
  $: if (videoElement && (selectedAudioTrack > 0 || needsAudioTranscoding) && audioPlayer) {
    videoElement.muted = true;
  } else if (videoElement && selectedAudioTrack === 0 && !needsAudioTranscoding) {
    // Unmute video element when using native audio (track 0) and no transcoding
    videoElement.muted = muted;
  }

  $: if (videoMetadata?.chapters) {
    chapters = videoMetadata.chapters;
  }

  $: seekChapter = chapters
    .filter((ch) => ch.start_time <= seekPreviewTime)
    .sort((a, b) => b.start_time - a.start_time)[0];

  // Initialize demuxer when src changes
  $: if (src && src !== lastInitializedSrc) {
    lastInitializedSrc = src;
    demuxerInitialized = false;
    hasSeekedToInitial = false;
    externalSubtitles = [];
    lastSubtitleFetchKey = null;
    selectedSubtitleTrack = -1;
    needsAudioTranscoding = false;
    metadataFetched = false;
    loadingAudio = false;
    loadingSubtitle = false;
    initializeDemuxer();
  }

  // Fetch external subtitles when media info is available
  $: {
    if (mediaId && mediaType) {
      const fetchKey = `${mediaId}-${mediaType}-${seasonNum}-${episodeNum}`;
      if (fetchKey !== lastSubtitleFetchKey) {
        loadExternalSubtitles(fetchKey);
      }
    }
  }

  async function loadExternalSubtitles(fetchKey) {
    if (loadingExternalSubs) return;
    loadingExternalSubs = true;
    lastSubtitleFetchKey = fetchKey;
    
    try {
      const subs = await fetchSubtitles(mediaId, mediaType, seasonNum, episodeNum);
      externalSubtitles = subs;
      console.log("loaded external subtitles:", externalSubtitles.length);
      
      // Try to apply subtitle preference if external subs were loaded
      if (subs.length > 0 && magnetLink) {
        setTimeout(() => loadTrackPreferences(), 100);
      }
    } catch (error) {
      console.error("failed to load external subtitles:", error);
      externalSubtitles = [];
    } finally {
      loadingExternalSubs = false;
    }
  }

  async function initializeDemuxer() {
    if (demuxerInitialized) return; // Already initialized or intentionally skipped
    demuxerInitialized = true;

    console.log("initializeDemuxer called with src:", src);
    console.log("metadata prop:", metadata);

    // Check if native audioTracks API is available (Safari/macOS)
    const hasNativeAudioTracks = videoElement && 'audioTracks' in videoElement;
    console.log("native audioTracks API available:", hasNativeAudioTracks);

    // Check if video source is from torrent streaming - fetch metadata from backend
    if (src && src.includes('/torrents/') && src.includes('/stream/')) {
      console.log("torrent stream detected, fetching metadata from backend");
      console.log("source URL:", src);
      loadingPhase = "metadata";
      loadingStatus.status = "Extracting video metadata...";
      loadingStatus.phaseProgress = 20;
      
      // Extract session_id and file_id from URL
      // URL format: http://localhost:PORT/torrents/{session_id}/stream/{file_id}
      const urlMatch = src.match(/\/torrents\/(\d+)\/stream\/(\d+)/);
      console.log("URL match result:", urlMatch);
      if (urlMatch) {
        torrentSessionId = parseInt(urlMatch[1]);
        torrentFileId = parseInt(urlMatch[2]);
        const baseUrl = src.substring(0, src.indexOf('/torrents/'));
        const portMatch = baseUrl.match(/:(\d+)$/);
        console.log("port match result:", portMatch);
        if (portMatch) {
          torrentHttpPort = parseInt(portMatch[1]);
        }
        console.log("parsed values - sessionId:", torrentSessionId, "fileId:", torrentFileId, "port:", torrentHttpPort);
        const metadataUrl = `${baseUrl}/torrents/${torrentSessionId}/metadata/${torrentFileId}`;
        
        console.log("fetching metadata from:", metadataUrl);
        
        try {
          loadingStatus.status = "Reading video container...";
          loadingStatus.phaseProgress = 40;
          
          const response = await fetch(metadataUrl);
          if (response.ok) {
            const fetchedMetadata = await response.json();
            console.log("fetched metadata:", fetchedMetadata);
            
            loadingStatus.status = "Processing track information...";
            loadingStatus.phaseProgress = 60;
            
            // Preserve transcoded_audio_url from previous videoMetadata if it exists
            const existingTranscodedUrl = videoMetadata?.transcoded_audio_url;
            
            // Set videoMetadata (merge with existing to preserve transcoded_audio_url)
            videoMetadata = {
              ...fetchedMetadata,
              // Keep transcoded_audio_url if we already have it
              transcoded_audio_url: existingTranscodedUrl || fetchedMetadata.transcoded_audio_url
            };
            chapters = fetchedMetadata.chapters || [];
            
            console.log(`found ${fetchedMetadata.audio_tracks.length} audio tracks`);
            console.log(`found ${fetchedMetadata.subtitle_tracks.length} subtitle tracks`);
            console.log(`found ${chapters.length} chapters`);
            console.log(`needs audio transcoding: ${fetchedMetadata.needs_audio_transcoding}`);
            console.log(`transcoded audio URL: ${videoMetadata.transcoded_audio_url}`);
            
            // Initialize demuxer for both subtitle and audio track extraction
            if (fetchedMetadata.subtitle_tracks.length > 0 || fetchedMetadata.audio_tracks.length > 1) {
              loadingPhase = "demuxing";
              loadingStatus.status = "Initializing demuxer...";
              loadingStatus.phaseProgress = 70;
              
              console.log("initializing MKV demuxer for subtitle/audio extraction");
              try {
                demuxer = new MKVDemuxer();
                console.log("[demuxer] MKV demuxer instance created, initializing with src:", src);
                loadingStatus.status = "Loading demuxer...";
                loadingStatus.phaseProgress = 80;
                await demuxer.initialize(src);
                loadingStatus.phaseProgress = 90;
                console.log("[demuxer] MKV demuxer initialized successfully");
                
                // Extract and save fonts from MKV if any (for ASS subtitles)
                // This is non-blocking and won't fail demuxer initialization
                try {
                  if (demuxer.attachments && demuxer.attachments.length > 0) {
                    loadingStatus.status = "Extracting fonts...";
                    loadingStatus.phaseProgress = 92;
                    console.log(`found ${demuxer.attachments.length} font attachments, extracting...`);
                    extractedFonts = await demuxer.extractAndSaveFonts();
                    console.log(`extracted ${extractedFonts.length} fonts:`, extractedFonts);
                  }
                } catch (fontError) {
                  console.warn("failed to extract fonts (non-fatal):", fontError);
                  extractedFonts = [];
                }
                
                loadingStatus.phaseProgress = 95;
              } catch (error) {
                console.error("[demuxer] failed to initialize MKV demuxer:", error);
                console.error("[demuxer] error details:", { message: error.message, stack: error.stack });
                demuxer = null;
                // Don't fail loading, just continue without demuxer
              }
            }
            
            loadingPhase = "ready";
            loadingStatus.status = "Ready to play";
            loadingStatus.phaseProgress = 100;
            loading = false;
            // Now trigger autoplay after all loading is complete
            startAutoplay();
            
            // Load and apply track preferences after everything is ready
            setTimeout(() => loadTrackPreferences(), 500);
          } else {
            console.error("failed to fetch metadata:", response.status, response.statusText);
            loadingPhase = "error";
            loadingStatus.status = "Error: Failed to load video metadata";
            loading = false;
          }
        } catch (error) {
          console.error("error fetching metadata:", error);
          loadingPhase = "error";
          loadingStatus.status = "Error: " + (error.message || "Failed to load metadata");
          loading = false;
        }
      } else {
        console.error("could not parse session_id and file_id from URL:", src);
        loadingPhase = "error";
        loadingStatus.status = "Error: Invalid stream URL";
        loading = false;
      }
      
      // Use native video element for playback
      useMkvDemuxer = false;
      if (videoElement) {
        videoElement.muted = false;
      }
      
      return;
    }

    // For non-torrent sources, use native video element
    console.log("using native video element playback");
    useMkvDemuxer = false;
    loadingPhase = "ready";
    loadingStatus.status = "Ready to play";
    loadingStatus.phaseProgress = 100;
    loading = false;
    
    // Unmute video element to use native audio
    if (videoElement) {
      videoElement.muted = false;
    }
    
    // Trigger autoplay for non-torrent sources
    startAutoplay();
    
    // If native audioTracks API exists, build metadata from it
    if (hasNativeAudioTracks && videoElement.audioTracks && videoElement.audioTracks.length > 0) {
      console.log("found", videoElement.audioTracks.length, "native audio tracks");
      metadata = {
        audio_tracks: Array.from(videoElement.audioTracks).map((track, index) => ({
          id: track.id,
          name: track.label || `Audio Track ${index + 1}`,
          language: track.language || 'und',
          enabled: track.enabled
        })),
        subtitle_tracks: videoElement.textTracks ? Array.from(videoElement.textTracks).map((track, index) => ({
          id: track.id,
          name: track.label || `Subtitle Track ${index + 1}`,
          language: track.language || 'und'
        })) : [],
        chapters: []
      };
      console.log("native track metadata:", metadata);
    }
  }

  async function startAutoplay() {
    // Wait a tick for loading overlay to hide
    await new Promise(resolve => setTimeout(resolve, 50));
    
    if (!videoElement) return;

    // Check if we need to use transcoded audio
    const hasTranscodedAudio = videoMetadata?.needs_audio_transcoding || videoMetadata?.transcoded_audio_url;

    console.log("=== startAutoplay ===");
    console.log("videoMetadata:", videoMetadata);
    console.log("needs_audio_transcoding:", videoMetadata?.needs_audio_transcoding);

    // Start muted to ensure autoplay works
    const wasMuted = videoElement.muted;
    videoElement.muted = true;
    await videoElement.play();
    console.log("video started playing (muted)");

    // If we have transcoded audio, stream it live from the backend
    if (hasTranscodedAudio && src.includes('/torrents/')) {
      console.log("setting up transcoded audio streaming");

      try {
        // Extract base URL and session ID from the video stream URL
        const urlMatch = src.match(/^(https?:\/\/[^\/]+)\/torrents\/(\d+)\/stream\//);
        if (!urlMatch) {
          throw new Error("Could not extract server URL from video source");
        }

        const baseUrl = urlMatch[1];
        const sessionId = urlMatch[2];

        // Construct the cached transcoding URL (background transcode + serve from cache)
        const transcodedStreamUrl = `${baseUrl}/torrents/${sessionId}/transcoded-audio-stream/${fileIndex}`;
        console.log("Transcoded audio stream URL:", transcodedStreamUrl);

        // Stop existing audio if any
        if (audioPlayer && audioPlayer instanceof Audio) {
          audioPlayer.pause();
          audioPlayer.src = '';
        }

        const targetStartTime = Number.isFinite(initialTimestamp) && initialTimestamp > 0
          ? initialTimestamp
          : videoElement.currentTime || 0;

        console.log("Target audio start time:", targetStartTime);

        // Setup audio element with transcoded stream
        prefetchedAudio = prefetchedAudio || new Audio();
        prefetchedAudio.preload = 'auto';
        prefetchedAudio.crossOrigin = 'anonymous';
        prefetchedAudio.src = transcodedStreamUrl;
        audioPlayer = prefetchedAudio;

        // Sync volume/mute
        audioPlayer.volume = volume;
        audioPlayer.muted = muted;
        videoElement.muted = true;

        // Add audio event listeners for buffering
        audioPlayer.addEventListener('waiting', () => {
          console.log('Audio waiting - showing buffer indicator');
          showBufferingIndicator = true;
          if (videoElement && !videoElement.paused) {
            videoElement.pause();
          }
        });
        
        audioPlayer.addEventListener('canplay', () => {
          console.log('Audio can play - hiding buffer indicator');
          showBufferingIndicator = false;
          if (playing && videoElement?.paused) {
            videoElement.play().catch(err => console.warn('video resume failed:', err));
          }
        });
        
        audioPlayer.addEventListener('stalled', () => {
          console.log('Audio stalled');
          showBufferingIndicator = true;
          if (videoElement && !videoElement.paused) {
            videoElement.pause();
          }
        });

        // Wait for metadata to set time
        await new Promise((resolve) => {
          if (audioPlayer.readyState >= 1) {
            resolve();
          } else {
            audioPlayer.addEventListener('loadedmetadata', resolve, { once: true });
          }
        });
        
        try {
          audioPlayer.currentTime = targetStartTime;
        } catch (e) {
          console.warn('Failed to set audio time:', e);
        }

        // Hide loading and start playback
        loading = false;
        
        await audioPlayer.play();
        
        // Resume video playback
        if (videoElement.paused && playing) {
          await videoElement.play();
        }

        selectedAudioTrack = 0;
        needsAudioTranscoding = true;
      } catch (audioErr) {
        console.error("Failed to setup transcoded audio streaming:", audioErr);
        // Fall back to native audio
        setTimeout(() => {
          if (videoElement && !wasMuted) {
            videoElement.muted = false;
          }
        }, 100);
      }
    } else {
      // No transcoded audio - unmute video after play starts successfully
      setTimeout(() => {
        if (videoElement && !wasMuted) {
          videoElement.muted = false;
        }
      }, 100);
    }

    playing = true;
  }

  function togglePlay() {
    if (showBufferingIndicator || waitingForAudio) return; // Ignore during buffering
    
    console.log("togglePlay called");
    const usingTranscodedAudio = needsAudioTranscoding && audioPlayer instanceof Audio;

    if (playing) {
      videoElement.pause();
      if ((selectedAudioTrack > 0 || usingTranscodedAudio) && audioPlayer instanceof Audio) {
        audioPlayer.pause();
      }
      playing = false;
    } else {
      if (videoElement.paused) {
        videoElement.play();
        if ((selectedAudioTrack > 0 || usingTranscodedAudio) && audioPlayer instanceof Audio) {
          audioPlayer.play();
        }
        playing = true;
      } else {
        videoElement.pause();
        playing = false;
      }
    }
  }

  async function startStreamProcess() {
    if (handleId === null || fileIndex === null) return;

    const numericHandle = Number(handleId);
    const numericFile = Number(fileIndex);

    loading = true;
    loadingPhase = "initializing";
    loadingStatus.status = "Preparing torrent stream...";
    loadingStatus.phaseProgress = 10;

    try {
      await invoke("prepare_stream", {
        handleId: numericHandle,
        fileIndex: numericFile,
      });
    } catch (error) {
      console.error("Failed to prepare stream:", error);
      loadingStatus.status = "Error preparing stream";
      loading = false;
      return;
    }

    const pollStatus = async () => {
      try {
        const status = await invoke("get_stream_status", {
          handleId: numericHandle,
          fileIndex: numericFile,
        });

        loadingStatus.progress = status.progress_bytes || 0;
        loadingStatus.total = status.total_bytes || 0;
        loadingStatus.peers = status.peers || 0;
        loadingStatus.speed = status.download_speed
          ? status.download_speed * 125000
          : 0;
        loadingStatus.transcodeProgress = status.transcode_progress;

        if (status.status === "transcoding") {
          loadingPhase = "transcoding";
          loadingStatus.status = "Transcoding audio...";
          loadingStatus.phaseProgress = 70;
        } else if (
          status.status === "ready" &&
          (loadingPhase === "initializing" || loadingPhase === "buffering")
        ) {
          loadingPhase = "buffering";
          loadingStatus.status = "Finalizing stream...";
          loadingStatus.phaseProgress = 90;
        } else if (loadingPhase === "initializing") {
          loadingPhase = "buffering";
          loadingStatus.status = "Buffering stream...";
          loadingStatus.phaseProgress = 50;
        }

        if (status.stream_info && status.stream_info.url) {
          if (!src) {
            src = status.stream_info.url;
          }
          if (!videoMetadata && status.stream_info.metadata) {
            videoMetadata = status.stream_info.metadata;
          }
        }

        if (status.status === "ready" && status.stream_info) {
          if (pollInterval) {
            clearInterval(pollInterval);
            pollInterval = null;
          }
        }
      } catch (error) {
        console.error("Failed to poll stream status:", error);
        loadingStatus.status = "Error loading stream";
      }
    };

    await pollStatus();
    if (!pollInterval) {
      pollInterval = setInterval(pollStatus, REFRESH_INTERVAL);
    }
  }

  function toggleMute() {
    muted = !muted;
    // Mute/unmute will be synced via reactive statements
  }

  function changeVolume(e) {
    volume = parseFloat(e.target.value);
    if (volume > 0) {
      muted = false;
      // HTML5 Audio syncs via reactive statements, no setMuted method
    }
    // Volume will be synced via reactive statements
  }

  async function toggleFullscreen() {
    const container = videoElement.closest(".video-player");
    const appWindow = getCurrentWindow();

    if (!fullscreen) {
      try {
        // Check if window is maximized and unmaximize it before entering fullscreen
        const isMaximized = await appWindow.isMaximized();
        wasMaximizedBeforeFullscreen = isMaximized;
        if (isMaximized) {
          await appWindow.unmaximize();
          // Wait for the window state to fully update
          await new Promise((resolve) => setTimeout(resolve, 200));
        }

        // Request fullscreen
        if (container.requestFullscreen) {
          await container.requestFullscreen();
        } else if (container.webkitRequestFullscreen) {
          await container.webkitRequestFullscreen();
        }
      } catch (err) {
        console.error("Fullscreen error:", err);
      }
    } else {
      try {
        // Exit fullscreen - restoration is handled by handleFullscreenChange
        if (document.exitFullscreen) {
          await document.exitFullscreen();
        } else if (document.webkitExitFullscreen) {
          await document.webkitExitFullscreen();
        }
      } catch (err) {
        console.error("Exit fullscreen error:", err);
      }
    }
  }

  function handleProgressHover(event) {
    if (!progressBar || !isFinite(duration)) return;
    const rect = progressBar.getBoundingClientRect();
    const ratio = Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1);
    hoverTime = ratio * duration;
    hoverX = event.clientX - rect.left;
    if (isSeeking) {
      seekPreviewTime = hoverTime;
    }
  }

  function handleProgressLeave() {
    hoverTime = null;
    hoverX = 0;
  }

  function startDrag(event) {
    isSeeking = true;
    document.body.style.userSelect = "none";
    handleProgressHover(event);
  }

  function handleDrag(event) {
    if (!isSeeking) return;
    handleProgressHover(event);
  }

  function stopDrag(event) {
    if (!isSeeking) return;
    isSeeking = false;
    document.body.style.userSelect = "";
    handleProgressHover(event);
    if (hoverTime !== null && videoElement && isFinite(duration)) {
      const newTime = Math.min(Math.max(hoverTime, 0), duration);
      currentTime = newTime; // Update immediately to prevent visual snap-back
      videoElement.currentTime = newTime;
      if (useMkvDemuxer) {
        if (demuxer) demuxer.seek(newTime, selectedAudioTrack);
        if (audioPlayer) audioPlayer.seek(newTime);
      }
    }
    justSeeked = true;
    setTimeout(() => {
      justSeeked = false;
    }, 500);
  }

  function handleTimeUpdate() {
    if (videoElement) {
      if (isFinite(videoElement.currentTime)) {
        currentTime = videoElement.currentTime;
      } else {
        console.warn("Video currentTime is not finite:", videoElement.currentTime);
      }
      
      if (isFinite(videoElement.duration)) {
        if (duration !== videoElement.duration) {
          console.log("Duration updated:", videoElement.duration);
        }
        duration = videoElement.duration;
      } else {
        console.warn("Video duration is not finite:", videoElement.duration);
      }
    }

    // SubtitlesOctopus updates automatically via video element binding
    // SRT renderer auto-updates via timeupdate event listener

    if (videoElement.buffered.length > 0) {
      buffered = videoElement.buffered.end(videoElement.buffered.length - 1);
    }
    
    // Sync HTML5 Audio playback with video (for extracted/transcoded audio tracks)
    const usingTranscodedAudio = needsAudioTranscoding && audioPlayer instanceof Audio;
    if ((selectedAudioTrack > 0 || usingTranscodedAudio) && audioPlayer && audioPlayer instanceof Audio && playing) {
      const drift = Math.abs(audioPlayer.currentTime - videoElement.currentTime);
      // Resync if drift exceeds 200ms
      if (drift > 0.2) {
        console.log(`Audio drift detected: ${drift.toFixed(3)}s, resyncing...`);
        audioPlayer.currentTime = videoElement.currentTime;
      }
    }
    
    // Check for skip sections in chapters
    checkSkipSections();
    
    // Track progress periodically (every 10 seconds)
    if (playing && !loading && currentTime > 0 && mediaId && mediaType) {
      if (!progressTrackingInterval) {
        progressTrackingInterval = setInterval(() => {
          if (playing && !loading && currentTime > 0 && mediaId && mediaType) {
            const progressData = {
              currentTimestamp: Math.floor(currentTime),
              duration: Math.floor(duration)
            };
            
            // Add season/episode for TV shows
            if (seasonNum !== null && episodeNum !== null) {
              progressData.currentSeason = seasonNum;
              progressData.currentEpisode = episodeNum;
            }
            
            watchProgressStore.updateProgress(mediaId, mediaType, progressData);
            
            // Add to watch history (only once when first playing)
            if (!watchHistoryAdded && metadata) {
              const historyItem = {
                id: mediaId,
                media_type: mediaType,
                title: metadata.title || metadata.name || 'Unknown',
                poster_path: metadata.poster_path,
                backdrop_path: metadata.backdrop_path,
                release_date: metadata.release_date || metadata.first_air_date,
                vote_average: metadata.vote_average,
                ...progressData
              };
              console.log('[Watch History] Adding item:', historyItem);
              watchHistoryStore.addItem(historyItem);
              watchHistoryAdded = true;
            }
          }
        }, 10000);
      }
    } else if (progressTrackingInterval) {
      // Clear interval when not playing
      clearInterval(progressTrackingInterval);
      progressTrackingInterval = null;
    }
  }

  function syncExternalAudio(targetTime) {
    if (audioPlayer && audioPlayer instanceof Audio) {
      const desiredTime = Number.isFinite(targetTime) ? targetTime : videoElement?.currentTime || 0;
      try {
        audioPlayer.currentTime = desiredTime;
        console.log(`[Audio Sync] Synced external audio to ${desiredTime.toFixed(3)}s`);
      } catch (e) {
        console.warn('Failed to sync external audio time:', e);
      }
    }
  }

  async function ensureTranscodedAudioPrepared(url, targetTime, existingAudio) {
    const audioEl = existingAudio || new Audio();
    audioEl.preload = 'auto';
    audioEl.crossOrigin = 'anonymous';
    
    // Only reload if URL changed
    if (audioEl.src !== url) {
      audioEl.src = url;
      audioEl.load();
    }

    const desiredTime = Number.isFinite(targetTime) ? targetTime : 0;

    // Wait for metadata and set position
    const waitForMetadata = () => new Promise((resolve) => {
      if (audioEl.readyState >= 1) { // HAVE_METADATA
        resolve();
      } else {
        audioEl.addEventListener('loadedmetadata', resolve, { once: true });
      }
    });
    
    await waitForMetadata();
    try { audioEl.currentTime = desiredTime; } catch (e) { /* best effort */ }
    
    // Don't wait for buffering - let browser handle it naturally
    // The cached file on disk will load progressively
    return audioEl;
  }

  function handleSeekingEvent() {
    // Sync audio for both transcoded AND extracted audio tracks
    const usingExternalAudio = (selectedAudioTrack > 0 || needsAudioTranscoding) && audioPlayer instanceof Audio;
    if (usingExternalAudio) {
      const targetTime = videoElement?.currentTime || 0;
      syncExternalAudio(targetTime);
    }

    // Clear SRT subtitle cache on seek for fresh data
    if (srtRenderer && srtRenderer.httpPort) {
      srtRenderer.clearCache();
    }
  }

  function handleWaitingEvent() {
    if (needsAudioTranscoding && audioPlayer instanceof Audio) {
      console.log('Video waiting - checking audio state');
      isBufferingSeek = true;
      waitingForAudio = true;
      showBufferingIndicator = true;
      
      // Pause video to wait for audio
      if (videoElement && !videoElement.paused) {
        videoElement.pause();
      }
    }
  }

  function handleCanPlayEvent() {
    if (!isBufferingSeek && !waitingForAudio) return;

    console.log('Video can play - checking audio state');
    
    // Ensure audio is ready before resuming
    if (needsAudioTranscoding && audioPlayer instanceof Audio) {
      if (audioPlayer.readyState < 2) { // HAVE_CURRENT_DATA
        console.log('Audio not ready yet, keeping buffer indicator');
        return; // Keep buffering
      }
      
      console.log('Audio ready, syncing and resuming');
      syncExternalAudio(videoElement?.currentTime);
      
      if (audioPlayer.paused) {
        audioPlayer.play().catch((err) => console.warn('Audio resume failed:', err));
      }
    }

    if (playing && videoElement?.paused) {
      videoElement.play().catch((err) => console.warn('Video resume failed:', err));
    }

    isBufferingSeek = false;
    waitingForAudio = false;
    showBufferingIndicator = false;
  }

  function checkSkipSections() {
    if (!chapters || chapters.length === 0 || !duration) return;

    // Find current chapter (using start_time property)
    let currentChapter = null;
    for (let i = 0; i < chapters.length; i++) {
      const chapter = chapters[i];
      const nextChapter = chapters[i + 1];
      const chapterStart = chapter.start_time;
      const chapterEnd = nextChapter ? nextChapter.start_time : duration;
      
      if (currentTime >= chapterStart && currentTime < chapterEnd) {
        currentChapter = { ...chapter, end_time: chapterEnd };
        break;
      }
    }

    // Check for skippable section
    if (currentChapter && currentChapter.title) {
      const chapterTitle = currentChapter.title.toLowerCase();
      const isSkippable = skipFilters.some(filter => chapterTitle.includes(filter));

      if (isSkippable && currentSkipSection?.title !== currentChapter.title && showSkipPrompts) {
        // New skippable section detected
        console.log('Skip section detected:', currentChapter.title);
        currentSkipSection = currentChapter;
        showSkipButton = true;
        skipTimerActive = true;
        skipTimeRemaining = 8;
        skipAnimationKey++; // Force animation restart
        
        // Clear existing timers
        if (skipButtonTimeout) clearTimeout(skipButtonTimeout);
        if (skipTimerInterval) clearInterval(skipTimerInterval);
        
        // Start countdown timer
        skipTimerInterval = setInterval(() => {
          skipTimeRemaining--;
          if (skipTimeRemaining <= 0) {
            clearInterval(skipTimerInterval);
            skipTimerInterval = null;
            skipTimerActive = false;
          }
        }, 1000);
        
        // Auto-hide button after 8 seconds (only if controls aren't shown)
        skipButtonTimeout = setTimeout(() => {
          if (!showControls) {
            showSkipButton = false;
          }
          skipButtonTimeout = null;
        }, 8000);
      }
    } else if (currentSkipSection) {
      // Left the skip section - always hide button
      currentSkipSection = null;
      showSkipButton = false;
      skipTimerActive = false;
      if (skipButtonTimeout) {
        clearTimeout(skipButtonTimeout);
        skipButtonTimeout = null;
      }
      if (skipTimerInterval) {
        clearInterval(skipTimerInterval);
        skipTimerInterval = null;
      }
    }

    // Check for next episode button (ending section)
    if (duration > 0 && seasonNum !== null && episodeNum !== null) {
      const endingThreshold = duration * 0.85; // Last 15% of video
      const lastChapter = chapters[chapters.length - 1];
      
      // Check if last chapter is in ending section AND duration is less than 15% of total
      if (lastChapter && lastChapter.start_time >= endingThreshold) {
        const lastChapterDuration = duration - lastChapter.start_time;
        const isShortEnding = lastChapterDuration <= (duration * 0.15);
        
        if (isShortEnding && currentTime >= lastChapter.start_time) {
          // In the short ending section
          if (!showNextEpisodeButton) {
            showNextEpisodeButton = true;
          }
        } else if (showNextEpisodeButton && currentTime < lastChapter.start_time) {
          showNextEpisodeButton = false;
        }
      } else if (showNextEpisodeButton) {
        showNextEpisodeButton = false;
      }
    }
  }

  function skipSection() {
    if (currentSkipSection && videoElement) {
      videoElement.currentTime = currentSkipSection.end_time;
      showSkipButton = false;
      skipTimerActive = false;
      currentSkipSection = null;
      if (skipButtonTimeout) clearTimeout(skipButtonTimeout);
      if (skipTimerInterval) clearInterval(skipTimerInterval);
    }
  }

  async function goToNextEpisode() {
    if (seasonNum === null || episodeNum === null) return;

    const nextEpisode = episodeNum + 1;
    
    // Check if next episode torrent is tracked
    try {
      const trackedTorrent = await invoke('get_saved_selection', {
        showId: Number(mediaId),
        season: seasonNum,
        episode: nextEpisode
      });

      if (trackedTorrent && trackedTorrent.magnet_link) {
        console.log('Found saved torrent for next episode:', trackedTorrent);
        
        // Close current player before loading next episode
        dispatch('close');
        
        // Add the torrent (VideoPlayer will handle preparation)
        const handleResult = await invoke('add_torrent', {
          magnetOrUrl: trackedTorrent.magnet_link
        });
        
        // Format title with season and episode
        const showName = metadata?.name || metadata?.title || title;
        const episodeTitle = `${showName} - S${seasonNum}E${nextEpisode}`;
        
        // Dispatch event to update video player with new episode
        // VideoPlayer will handle stream preparation and show proper loading phases
        window.dispatchEvent(
          new CustomEvent('openVideoPlayer', {
            detail: {
              src: null, // Let VideoPlayer fetch the stream URL
              title: episodeTitle,
              metadata: metadata,
              handleId: handleResult,
              fileIndex: trackedTorrent.file_index,
              magnetLink: trackedTorrent.magnet_link,
              initialTimestamp: 0,
              mediaId: mediaId,
              mediaType: mediaType,
              seasonNum: seasonNum,
              episodeNum: nextEpisode,
            },
          }),
        );
      } else {
        // Go back to media details to select torrent for next episode
        console.log('No saved torrent found, navigating to torrent selector');
        window.location.hash = `/media/${mediaType}/${mediaId}?season=${seasonNum}&episode=${nextEpisode}`;
      }
    } catch (error) {
      console.error('Error navigating to next episode:', error);
      // Fallback to torrent selector
      window.location.hash = `/media/${mediaType}/${mediaId}?season=${seasonNum}&episode=${nextEpisode}`;
    }
  }

  function handleLoadedMetadata() {
    console.log("=== handleLoadedMetadata called ===");
    duration = videoElement.duration;
    
    // Check if audio track switching is supported
    audioTrackSwitchingSupported = videoElement && 'audioTracks' in videoElement && 
                                   videoElement.audioTracks && videoElement.audioTracks.length > 1;
    console.log("Audio track switching supported:", audioTrackSwitchingSupported, 
                "(tracks:", videoElement.audioTracks?.length || 0, ")");
    
    console.log("Duration:", duration);
    console.log("Initial timestamp:", initialTimestamp);
    console.log("Has seeked:", hasSeekedToInitial);
    console.log("Current time before seek:", videoElement.currentTime);

    // Seek to initial timestamp if provided and not already seeked
    if (initialTimestamp > 0 && !hasSeekedToInitial && isFinite(initialTimestamp) && isFinite(duration)) {
      console.log("attempting to seek to initial timestamp:", initialTimestamp);
      videoElement.currentTime = Math.min(initialTimestamp, duration);
      hasSeekedToInitial = true;
      console.log("Immediately after setting currentTime:", videoElement.currentTime);
      // Verify seek after a short delay
      setTimeout(() => {
        console.log("Seek verification - Current time:", videoElement.currentTime, "Target was:", initialTimestamp, "Difference:", Math.abs(videoElement.currentTime - initialTimestamp));
      }, 100);
    } else if (initialTimestamp > 0) {
      console.log("skipping seek - already seeked to initial timestamp");
    } else {
      console.log("no initial timestamp to seek to (initialTimestamp:", initialTimestamp, ")");
    }

    // Keep external audio aligned after metadata is ready
    syncExternalAudio(videoElement.currentTime);

    // SubtitlesOctopus automatically manages canvas size based on video dimensions
  }

  async function handleFullscreenChange() {
    const wasFullscreen = fullscreen;
    fullscreen = !!(
      document.fullscreenElement || document.webkitFullscreenElement
    );
    
    // If exiting fullscreen and window was maximized before, restore it
    if (wasFullscreen && !fullscreen && wasMaximizedBeforeFullscreen) {
      try {
        const appWindow = getCurrentWindow();
        // Give more time for fullscreen exit to complete
        await new Promise((resolve) => setTimeout(resolve, 300));
        await appWindow.maximize();
        wasMaximizedBeforeFullscreen = false;
        console.log("Window maximized after exiting fullscreen");
      } catch (err) {
        console.error("Error restoring maximize state:", err);
      }
    }
  }

  function close() {
    dispatch("close");
  }

  // formatTime moved to src/lib/utils/timeUtils.js

  function handleMouseMove() {
    if (!showControls) {
      showControls = true;
      window.dispatchEvent(new CustomEvent("videoControlsVisibility", { detail: { visible: true } }));
    }
    clearTimeout(controlsTimeout);
    controlsTimeout = setTimeout(() => {
      // Don't hide controls if any menu is open
      if (playing && !isSeeking && !showPlayerMenu && !showChaptersMenu && !showAudioSubmenu && !showSubtitleSubmenu) {
        showControls = false;
        window.dispatchEvent(new CustomEvent("videoControlsVisibility", { detail: { visible: false } }));
      }
    }, CONTROLS_HIDE_TIMEOUT);
  }

  async function selectAudioTrack(index) {
    console.log("[Audio] selectAudioTrack called:", { index, currentlySelected: selectedAudioTrack, loading: loadingAudio });
    
    const previousTrack = selectedAudioTrack;
    selectedAudioTrack = index;
    loadingAudio = true;

    try {
      // Track 0 is always the default native audio - just unmute video and stop AudioPlayer
      if (index === 0) {
        console.log("switching to default (native) audio track");
        
        // Stop and cleanup HTML5 Audio if active
        if (audioPlayer && audioPlayer instanceof Audio) {
          audioPlayer.pause();
          audioPlayer.src = '';
          audioPlayer = null;
        }
        
        // Unmute video element
        if (videoElement) {
          videoElement.muted = false;
        }
        
        selectedAudioTrack = index;
        showAudioMenu = false;
        loadingAudio = false;
        saveTrackPreferences();
        return;
      }

      // For other tracks, try native switching first
      if (videoElement && 'audioTracks' in videoElement && videoElement.audioTracks && videoElement.audioTracks.length > 0) {
        console.log("Attempting native audioTracks API, available tracks:", videoElement.audioTracks.length);
        try {
          for (let i = 0; i < videoElement.audioTracks.length; i++) {
            videoElement.audioTracks[i].enabled = false;
          }
          if (videoElement.audioTracks[index]) {
            videoElement.audioTracks[index].enabled = true;
            console.log(`switched to native audio track ${index}`);
            selectedAudioTrack = index;
            showAudioMenu = false;
            loadingAudio = false;
            saveTrackPreferences();
            return;
          }
        } catch (error) {
          console.error('Error switching native audio track:', error);
        }
      }

      // Native switching not available - extract and cache audio to disk
      console.log("native audioTracks API not available, using audio file caching");
      
      if (!demuxer) {
        throw new Error('Demuxer not available for audio extraction');
      }

      if (!videoMetadata?.audio_tracks?.[index]) {
        throw new Error(`Audio track ${index} not found in metadata`);
      }

      // Check cache first
      const stableCacheId = getStableCacheId();
      let audioBlobUrl = null;

      // Determine MIME type based on codec
      const trackInfo = videoMetadata.audio_tracks[index];
      const codec = trackInfo.codec.toLowerCase();
      let mimeType = 'audio/webm';
      
      if (codec === 'aac') {
        mimeType = 'audio/aac';
      } else if (codec === 'mp3' || codec === 'mp3float') {
        mimeType = 'audio/mpeg';
      } else if (codec === 'opus') {
        mimeType = 'audio/ogg; codecs=opus';
      } else if (codec === 'vorbis') {
        mimeType = 'audio/ogg; codecs=vorbis';
      } else if (codec === 'flac') {
        mimeType = 'audio/flac';
      }
      
      console.log(`Audio codec: ${codec}, using MIME type: ${mimeType}`);

      // Check filesystem cache
      const cachedAudio = await loadCachedAudio(stableCacheId, fileIndex, index);
      
      if (cachedAudio) {
        console.log(`[Audio Cache] Filesystem HIT - Loaded from disk`);
        // Use MKV container format (all audio is now extracted as MKV)
        const blob = new Blob([cachedAudio], { type: 'video/x-matroska' });
        const blobUrl = URL.createObjectURL(blob);
        
        // Stop existing audio
        if (audioPlayer && audioPlayer instanceof Audio) {
          audioPlayer.pause();
          audioPlayer.src = '';
        }
        
        // Create and prepare audio element
        audioPlayer = new Audio();
        audioPlayer.preload = 'auto';
        audioPlayer.crossOrigin = 'anonymous';
        audioPlayer.src = blobUrl;
        audioPlayer.volume = volume;
        audioPlayer.muted = muted;
        
        // Mute video
        if (videoElement) {
          videoElement.muted = true;
        }
        
        // Wait for audio to be ready
        showBufferingIndicator = true;
        await ensureTranscodedAudioPrepared(blobUrl, videoElement?.currentTime || 0, audioPlayer);
        
        // Sync and play
        syncExternalAudio(videoElement?.currentTime || 0);
        if (!videoElement.paused) {
          await audioPlayer.play();
        }
        showBufferingIndicator = false;
        saveTrackPreferences();
      } else {
        console.log(`[Audio Cache] MISS - Extracting and saving audio`);
        // Extract audio using backend FFmpeg (properly remuxed)
        showBufferingIndicator = true;
        
        try {
          // Use backend to extract audio track with proper remuxing
          const audioData = await invoke("extract_audio_track", {
            handleId: handleId,
            fileIndex: fileIndex,
            trackIndex: index
          });
          
          console.log(`[Audio Cache] Received ${audioData.length} bytes from backend`);
          
          // Convert to Uint8Array (Tauri returns Vec<u8> as regular array)
          const audioBuffer = new Uint8Array(audioData);
          
          // Save to filesystem cache
          console.log(`[Audio Cache] STORE - Saving ${audioBuffer.length} bytes to disk`);
          await saveCachedAudio(stableCacheId, fileIndex, index, audioBuffer);
          
          // Create blob with proper MIME type for MKV container
          const blob = new Blob([audioBuffer], { type: 'video/x-matroska' });
          const blobUrl = URL.createObjectURL(blob);
          
          // Stop existing audio
          if (audioPlayer && audioPlayer instanceof Audio) {
            audioPlayer.pause();
            audioPlayer.src = '';
          }
          
          // Create and prepare audio element
          audioPlayer = new Audio();
          audioPlayer.preload = 'auto';
          audioPlayer.crossOrigin = 'anonymous';
          audioPlayer.src = blobUrl;
          audioPlayer.volume = volume;
          audioPlayer.muted = muted;
          
          // Mute video
          if (videoElement) {
            videoElement.muted = true;
          }
          
          // Wait for audio to be ready
          await ensureTranscodedAudioPrepared(blobUrl, videoElement?.currentTime || 0, audioPlayer);
          
          // Sync and play
          syncExternalAudio(videoElement?.currentTime || 0);
          if (!videoElement.paused) {
            await audioPlayer.play();
          }
          showBufferingIndicator = false;
          saveTrackPreferences();
        } catch (extractError) {
          console.error("Backend audio extraction failed:", extractError);
          showBufferingIndicator = false;
          throw extractError;
        }
      }

      console.log(`switched to cached audio track ${index} using HTML5 audio`);
    } catch (error) {
      console.error("Failed to switch audio track:", error);
      selectedAudioTrack = previousTrack;
    } finally {
      loadingAudio = false;
      showAudioMenu = false;
    }
  }

  async function selectSubtitle(track, trackIndex) {
    console.log('[Subtitle] selectSubtitle called:', { track, trackIndex, currentlySelected: selectedSubtitleTrack, loading: loadingSubtitle });
    
    selectedSubtitleTrack = trackIndex;
    loadingSubtitle = true;

    try {
      // Hide any previously active subtitle renderers
      if (subtitleRenderer) {
        subtitleRenderer.hide();
      }
      // Note: Don't hide srtRenderer yet - we might be selecting an SRT subtitle

      // Save track preference
      saveTrackPreferences();
      
      // Handle external subtitles from Wyzie
      if (track.source === "wyzie") {
        // Use streaming fetcher for Wyzie SRT subtitles
        const stableCacheId = getStableCacheId();
        const trackCacheIndex = 8000 + trackIndex; // Prefix to avoid collisions with MKV tracks
        
        streamingSrtFetcher = new StreamingSrtFetcher(
          track.url,
          stableCacheId,
          fileIndex,
          trackCacheIndex,
          {
            load: loadCachedSubtitle,
            save: saveCachedSubtitle
          }
        );
        
        // Initialize fetcher (loads from cache or fetches)
        await streamingSrtFetcher.initialize();
        
        if (!srtRenderer && videoElement) {
          srtRenderer = new SRTSubtitleRenderer(videoElement);
          srtRenderer.initialize();
        }
        
        if (srtRenderer) {
          const subtitles = streamingSrtFetcher.getSubtitles();
          // Pass the fetcher to the renderer for streaming updates
          srtRenderer.setStreamingFetcher(streamingSrtFetcher);
          srtRenderer.setSubtitles(subtitles);
          srtRenderer.show();
        }
      } else if (demuxer) {
        // Determine codec from track info first to decide extraction method
        const subtitleTrack = demuxer.subtitleTracks[trackIndex];
        const codec = subtitleTrack?.codec?.toLowerCase() || 'ass';
        console.log(`[Subtitle] Loading subtitle with codec: ${codec}`);
        
        // For SRT/SubRip subtitles, use demuxer streaming (extract around playhead)
        if (codec === 'srt' || codec === 'subrip' || codec === 'sub') {
          // Hide ASS renderer if active
          if (subtitleRenderer) {
            subtitleRenderer.hide();
          }
          
          if (!srtRenderer && videoElement) {
            srtRenderer = new SRTSubtitleRenderer(videoElement);
            srtRenderer.initialize();
          }
          
          if (srtRenderer && demuxer) {
            console.log('[Subtitle] Setting up streaming SRT subtitle extraction via demuxer');
            // Set up streaming fetcher that uses demuxer
            srtRenderer.setDemuxerStreaming(demuxer, trackIndex, duration);
            srtRenderer.show();
            
            // Start background task to extract complete subtitles for caching
            const stableCacheId = getStableCacheId();
            extractCompleteSubtitleInBackground(stableCacheId, fileIndex, trackIndex);
          }
        } else {
          // For ASS/SSA subtitles, use full extraction with caching
          // Hide SRT renderer if active
          if (srtRenderer) {
            srtRenderer.hide();
            srtRenderer.setStreamingFetcher(null);
          }
          
          // Use stable cache ID based on magnet link info hash
          const stableCacheId = getStableCacheId();
          const cacheKey = `${stableCacheId}-${fileIndex}-${trackIndex}`;
          let subtitleData = subtitleCache[cacheKey];
          
          console.log(`[Subtitle Cache] Using stable cache ID: ${stableCacheId}`);
          
          if (subtitleData) {
            console.log(`[Subtitle Cache] Memory HIT - Using cached subtitle for key: ${cacheKey}`);
          } else {
            // Check filesystem cache
            subtitleData = await loadCachedSubtitle(stableCacheId, fileIndex, trackIndex);
            
            if (subtitleData) {
              console.log(`[Subtitle Cache] Filesystem HIT - Loaded from disk for key: ${cacheKey}`);
              // Store in memory cache for faster access
              subtitleCache[cacheKey] = subtitleData;
            } else {
              console.log(`[Subtitle Cache] MISS - Extracting subtitle for key: ${cacheKey}`);
              // Extract subtitle from demuxer
              subtitleData = await demuxer.extractSubtitleTrack(trackIndex);
              
              if (!subtitleData) {
                throw new Error('No subtitle data extracted');
              }
              
              console.log(`[Subtitle Cache] STORE - Cached ${subtitleData.length} bytes for key: ${cacheKey}`);
              // Cache in memory and filesystem
              subtitleCache[cacheKey] = subtitleData;
              await saveCachedSubtitle(stableCacheId, fileIndex, trackIndex, subtitleData);
            }
          }
          
          // Embed fonts into ASS data if we have extracted fonts
          if ((codec === 'ass' || codec === 'ssa') && extractedFonts && extractedFonts.length > 0) {
            console.log('[Subtitle] Embedding fonts into ASS data');
            subtitleData = await demuxer.embedFontsIntoASS(subtitleData, extractedFonts);
          }
          
          // Hide SRT renderer if active
          // Hide SRT renderer if active
          if (srtRenderer) {
            srtRenderer.hide();
          }
          
          // Use SubtitlesOctopus for ASS/SSA
          // Create renderer if not exists, or use existing one (it will reinitialize internally)
          if (!subtitleRenderer) {
            subtitleRenderer = new SubtitleRenderer(null, videoElement);
          }
          
          // Set extracted fonts served via HTTP backend
          if (extractedFonts && extractedFonts.length > 0) {
            const fontUrls = extractedFonts
              .filter(f => f.url && !f.skipped)
              .map(f => f.url);
            if (fontUrls.length > 0) {
              console.log('[Subtitle] Setting extracted fonts via HTTP:', fontUrls);
              subtitleRenderer.setFonts(fontUrls);
            }
          }
          
          // loadSubtitleTrack will reinitialize the octopus instance for proper track switching
          await subtitleRenderer.loadSubtitleTrack(subtitleData, codec);
          subtitleRenderer.show();
          
          // Seek backward slightly to fix ASS subtitle stutter
          if (videoElement && videoElement.currentTime > 0.25) {
            const currentTime = videoElement.currentTime;
            videoElement.currentTime = currentTime - 0.25;
            console.log(`[Subtitle] Seeked back 0.25s to fix stutter (${currentTime} → ${currentTime - 0.25})`);
          }
        }
      } else {
        // Demuxer not available for embedded subtitle extraction
        console.error('[Subtitle] Demuxer is null or undefined:', { demuxer, trackSource: track?.source });
        throw new Error('Demuxer not initialized. Cannot extract embedded subtitles.');
      }
    } catch (error) {
      console.error("Failed to load subtitle:", error);
    } finally {
      loadingSubtitle = false;
      showSubtitleMenu = false;
    }
  }

  function disableSubtitles() {
    selectedSubtitleTrack = -1;

    if (subtitleRenderer) {
      subtitleRenderer.hide();
    }
    
    if (srtRenderer) {
      srtRenderer.hide();
    }

    showSubtitleMenu = false;
  }

  function jumpToChapter(startTime) {
    if (videoElement && isFinite(startTime) && isFinite(duration)) {
      videoElement.currentTime = Math.min(startTime, duration);
      console.log(`Jumped to chapter at ${formatTime(startTime)}`);
      
      if (useMkvDemuxer) {
        if (demuxer) demuxer.seek(startTime, selectedAudioTrack);
        if (audioPlayer) audioPlayer.seek(startTime);
      }
    }
    showChaptersMenu = false;
  }

  async function openInExternalPlayer() {
    try {
      // Get player setting from backend
      const settings = await invoke('get_settings');
      const externalPlayer = settings.external_player || 'mpv';
      
      // Check if player is installed
      const installed = await invoke('check_external_player', { player: externalPlayer });
      
      if (!installed) {
        alert(`${externalPlayer.toUpperCase()} is not installed or not in PATH. Please install it to use external playback.`);
        return;
      }
      
      // Open stream in external player
      await invoke('open_in_external_player', {
        player: externalPlayer,
        streamUrl: src,
        title: title
      });
      
      // Switch to external player mode
      playingInExternal = true;
      if (playing) {
        videoElement?.pause();
      }
      showPlayerMenu = false;
      
    } catch (error) {
      console.error('Failed to open in external player:', error);
      alert(`Failed to open external player: ${error}`);
    }
  }

  function restoreInternalPlayer() {
    playingInExternal = false;
    // Video will auto-resume if it was playing
  }

  function togglePlayerMenu() {
    showPlayerMenu = !showPlayerMenu;
    if (showPlayerMenu) {
      showChaptersMenu = false;
      showAudioSubmenu = false;
      showSubtitleSubmenu = false;
    } else {
      showAudioSubmenu = false;
      showSubtitleSubmenu = false;
    }
  }

  function toggleAudioSubmenu(event) {
    const wasOpen = showAudioSubmenu;
    showAudioSubmenu = !showAudioSubmenu;
    showSubtitleSubmenu = false;
    
    if (showAudioSubmenu) {
      const button = event.currentTarget;
      const buttonRect = button.getBoundingClientRect();
      
      // Store button position for initial render
      audioSubmenuX = buttonRect.left - 316;
      audioSubmenuY = buttonRect.top - 8;
      
      // Wait for submenu to render, then position it accurately
      setTimeout(() => {
        if (audioSubmenuElement) {
          const submenuWidth = audioSubmenuElement.offsetWidth;
          const submenuHeight = audioSubmenuElement.offsetHeight;
          
          // Position submenu to the left of the button with gap
          audioSubmenuX = buttonRect.left - submenuWidth - 16;
          
          // Align with button top, constrained to viewport
          const minY = 20;
          const maxY = window.innerHeight - submenuHeight - 20;
          audioSubmenuY = Math.max(minY, Math.min(buttonRect.top, maxY)) - 8;
          
          console.log('Audio submenu position:', { x: audioSubmenuX, y: audioSubmenuY, submenuWidth, buttonLeft: buttonRect.left });
        }
      }, 0);
    }
  }

  function toggleSubtitleSubmenu(event) {
    const wasOpen = showSubtitleSubmenu;
    showSubtitleSubmenu = !showSubtitleSubmenu;
    showAudioSubmenu = false;
    
    if (showSubtitleSubmenu) {
      const button = event.currentTarget;
      const buttonRect = button.getBoundingClientRect();
      
      // Store button position for initial render
      subtitleSubmenuX = buttonRect.left - 316;
      subtitleSubmenuY = buttonRect.top - 8;
      
      // Wait for submenu to render, then position it accurately
      setTimeout(() => {
        if (subtitleSubmenuElement) {
          const submenuWidth = subtitleSubmenuElement.offsetWidth;
          const submenuHeight = subtitleSubmenuElement.offsetHeight;
          
          // Position submenu to the left of the button with gap
          subtitleSubmenuX = buttonRect.left - submenuWidth - 16;
          
          // Align with button top, constrained to viewport
          const minY = 20;
          const maxY = window.innerHeight - submenuHeight - 20;
          subtitleSubmenuY = Math.max(minY, Math.min(buttonRect.top, maxY)) - 8;
          
          console.log('Subtitle submenu position:', { x: subtitleSubmenuX, y: subtitleSubmenuY, submenuWidth, buttonLeft: buttonRect.left });
        }
      }, 0);
    }
  }

  async function toggleSkipPrompts() {
    showSkipPrompts = !showSkipPrompts;
    try {
      const settings = await invoke('get_settings');
      settings.show_skip_prompts = showSkipPrompts;
      await invoke('save_settings', { settings });
      console.log('saved showSkipPrompts to backend:', showSkipPrompts);
    } catch (error) {
      console.error('failed to save showSkipPrompts:', error);
    }
  }

  async function goToNextEpisodeMenu() {
    if (seasonNum === null || episodeNum === null) return;

    const nextEpisode = episodeNum + 1;
    
    // Check if next episode torrent is tracked
    try {
      const trackedTorrent = await invoke('get_saved_selection', {
        showId: Number(mediaId),
        season: seasonNum,
        episode: nextEpisode
      });

      if (trackedTorrent && trackedTorrent.magnet_link) {
        console.log('Found saved torrent for next episode:', trackedTorrent);
        
        // Close current player before loading next episode
        dispatch('close');
        
        // Add the torrent (VideoPlayer will handle preparation)
        const handleResult = await invoke('add_torrent', {
          magnetOrUrl: trackedTorrent.magnet_link
        });
        
        // Format title with season and episode
        const showName = metadata?.name || metadata?.title || title;
        const episodeTitle = `${showName} - S${seasonNum}E${nextEpisode}`;
        
        // Dispatch event to update video player with new episode
        // VideoPlayer will handle stream preparation and show proper loading phases
        window.dispatchEvent(
          new CustomEvent('openVideoPlayer', {
            detail: {
              src: null, // Let VideoPlayer fetch the stream URL
              title: episodeTitle,
              metadata: metadata,
              handleId: handleResult,
              fileIndex: trackedTorrent.file_index,
              magnetLink: trackedTorrent.magnet_link,
              initialTimestamp: 0,
              mediaId: mediaId,
              mediaType: mediaType,
              seasonNum: seasonNum,
              episodeNum: nextEpisode,
            },
          }),
        );
      } else {
        // Go back to media details to select torrent for next episode
        console.log('No saved torrent found, navigating to torrent selector');
        window.location.hash = `/media/${mediaType}/${mediaId}?season=${seasonNum}&episode=${nextEpisode}`;
      }
    } catch (error) {
      console.error('Error navigating to next episode:', error);
      // Fallback to torrent selector
      window.location.hash = `/media/${mediaType}/${mediaId}?season=${seasonNum}&episode=${nextEpisode}`;
    }
  }

  function showShortcutIndicator(type, value, icon, seekAmount = 0, volumeDirection = null) {
    // Clear existing timeout to prevent overlays
    if (indicatorTimeout) {
      clearTimeout(indicatorTimeout);
    }
    
    // Determine if this is a repeated action (nudge) or new action (appear)
    const isRepeatedAction = showIndicator && lastIndicatorType === type;
    
    // Handle seek stacking
    if (type === 'seek-forward' || type === 'seek-backward') {
      const direction = type === 'seek-forward' ? 'forward' : 'backward';
      
      // Reset accumulator if direction changed or indicator was hidden
      if (!showIndicator || lastSeekDirection !== direction) {
        seekAccumulator = 0;
        lastSeekDirection = direction;
      }
      
      seekAccumulator += seekAmount;
      value = `${seekAccumulator >= 0 ? '+' : ''}${seekAccumulator}s`;
    } else {
      // Reset seek accumulator for non-seek indicators
      seekAccumulator = 0;
      lastSeekDirection = null;
    }
    
    // For volume, use direction-specific type for proper animation
    if (type === 'volume' && volumeDirection) {
      indicatorType = volumeDirection === 'up' ? 'volume-up' : 'volume-down';
    } else {
      indicatorType = type;
    }
    indicatorValue = value;
    indicatorIcon = icon;
    lastIndicatorType = type;
    
    if (isRepeatedAction) {
      // Just nudge, don't restart appear animation
      indicatorNudgeKey++;
    } else {
      // New action, restart full animation
      indicatorAnimationKey++;
    }
    
    showIndicator = true;
    
    // Auto-hide after 600ms
    indicatorTimeout = setTimeout(() => {
      showIndicator = false;
      seekAccumulator = 0;
      lastSeekDirection = null;
      lastIndicatorType = null;
    }, 600);
  }

  function handleKeyPress(event) {
    // Don't handle if user is typing in an input
    if (
      event.target.tagName === "INPUT" ||
      event.target.tagName === "TEXTAREA"
    ) {
      return;
    }

    switch (event.key.toLowerCase()) {
      case " ":
      case "k":
        event.preventDefault();
        togglePlay();
        showShortcutIndicator(
          playing ? "play" : "pause",
          playing ? "Play" : "Pause",
          playing ? "ri-play-fill" : "ri-pause-fill"
        );
        break;
      case "arrowleft":
        event.preventDefault();
        if (videoElement && isFinite(videoElement.currentTime)) {
          const newTime = Math.max(
            0,
            videoElement.currentTime - SEEK_TIME_SHORT,
          );
          videoElement.currentTime = newTime;
          if (useMkvDemuxer) {
            if (demuxer) demuxer.seek(newTime, selectedAudioTrack);
            if (audioPlayer) audioPlayer.seek(newTime);
          }
          showShortcutIndicator("seek-backward", "-5s", "ri-rewind-fill", -5);
        }
        break;
      case "arrowright":
        event.preventDefault();
        if (videoElement && isFinite(videoElement.currentTime) && isFinite(duration)) {
          const newTime = Math.min(
            duration,
            videoElement.currentTime + SEEK_TIME_SHORT,
          );
          videoElement.currentTime = newTime;
          if (useMkvDemuxer) {
            if (demuxer) demuxer.seek(newTime, selectedAudioTrack);
            if (audioPlayer) audioPlayer.seek(newTime);
          }
          showShortcutIndicator("seek-forward", "+5s", "ri-speed-fill", 5);
        }
        break;
      case "j":
        event.preventDefault();
        if (videoElement && isFinite(videoElement.currentTime)) {
          const newTime = Math.max(
            0,
            videoElement.currentTime - SEEK_TIME_LONG,
          );
          videoElement.currentTime = newTime;
          if (useMkvDemuxer) {
            if (demuxer) demuxer.seek(newTime, selectedAudioTrack);
            if (audioPlayer) audioPlayer.seek(newTime);
          }
          showShortcutIndicator("seek-backward", "-10s", "ri-rewind-fill", -10);
        }
        break;
      case "l":
        event.preventDefault();
        if (videoElement && isFinite(videoElement.currentTime) && isFinite(duration)) {
          const newTime = Math.min(
            duration,
            videoElement.currentTime + SEEK_TIME_LONG,
          );
          videoElement.currentTime = newTime;
          if (useMkvDemuxer) {
            if (demuxer) demuxer.seek(newTime, selectedAudioTrack);
            if (audioPlayer) audioPlayer.seek(newTime);
          }
          showShortcutIndicator("seek-forward", "+10s", "ri-speed-fill", 10);
        }
        break;
      case "arrowup":
        event.preventDefault();
        volume = Math.min(1, volume + VOLUME_STEP_SMALL);
        if (volume > 0 && muted) {
          muted = false;
        }
        const iconUp = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
        showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconUp, 0, 'up');
        break;
      case "arrowdown":
        event.preventDefault();
        volume = Math.max(0, volume - VOLUME_STEP_SMALL);
        const iconDown = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
        showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconDown, 0, 'down');
        break;
      case "u":
        event.preventDefault();
        volume = Math.max(0, volume - VOLUME_STEP_LARGE);
        const iconU = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
        showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconU, 0, 'down');
        break;
      case "m":
        event.preventDefault();
        toggleMute();
        showShortcutIndicator(
          muted ? "mute" : "unmute",
          muted ? "Muted" : "Unmuted",
          muted ? "ri-volume-mute-fill" : "ri-volume-up-fill"
        );
        break;
      case "f":
        event.preventDefault();
        toggleFullscreen();
        showShortcutIndicator(
          fullscreen ? "exit-fullscreen" : "fullscreen",
          fullscreen ? "Exit Fullscreen" : "Fullscreen",
          fullscreen ? "ri-fullscreen-exit-fill" : "ri-fullscreen-fill"
        );
        break;
      case "enter":
        event.preventDefault();
        if (showSkipButton && currentSkipSection) {
          skipSection();
        } else if (showNextEpisodeButton) {
          goToNextEpisode();
        }
        break;
    }
  }

  onMount(async () => {
    console.log("VideoPlayer mounted");
    
    // Load settings from backend
    try {
      const settings = await invoke('get_settings');
      showSkipPrompts = settings.show_skip_prompts;
      console.log('Loaded showSkipPrompts from backend:', showSkipPrompts);
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
    
    document.addEventListener("fullscreenchange", handleFullscreenChange);
    document.addEventListener("webkitfullscreenchange", handleFullscreenChange);
    window.addEventListener("mousemove", handleDrag);
    window.addEventListener("mouseup", stopDrag);
    window.addEventListener("keydown", handleKeyPress);

    // Initialize SRT subtitle renderer for non-demuxer playback
    if (videoElement) {
      srtRenderer = new SRTSubtitleRenderer(videoElement);
      srtRenderer.initialize();
    }

    // Periodic check for skip section state (catches edge cases like keyboard seeking)
    skipSectionCheckInterval = setInterval(() => {
      if (currentSkipSection && videoElement) {
        const stillInSection = videoElement.currentTime >= currentSkipSection.start_time && 
                               videoElement.currentTime < currentSkipSection.end_time;
        if (!stillInSection) {
          currentSkipSection = null;
          showSkipButton = false;
          skipTimerActive = false;
          if (skipButtonTimeout) {
            clearTimeout(skipButtonTimeout);
            skipButtonTimeout = null;
          }
          if (skipTimerInterval) {
            clearInterval(skipTimerInterval);
            skipTimerInterval = null;
          }
        }
      }
    }, 500);

    if (handleId !== null && fileIndex !== null) {
      startStreamProcess();
    } else {
      loading = false;
    }
  });

  onDestroy(async () => {
    clearInterval(pollInterval);
    if (progressTrackingInterval) {
      clearInterval(progressTrackingInterval);
    }
    if (skipButtonTimeout) {
      clearTimeout(skipButtonTimeout);
    }
    if (skipTimerInterval) {
      clearInterval(skipTimerInterval);
    }
    if (skipSectionCheckInterval) {
      clearInterval(skipSectionCheckInterval);
    }
    skipTimerActive = false;
    document.removeEventListener("fullscreenchange", handleFullscreenChange);
    document.removeEventListener(
      "webkitfullscreenchange",
      handleFullscreenChange,
    );
    window.removeEventListener("mousemove", handleDrag);
    window.removeEventListener("mouseup", stopDrag);
    window.removeEventListener("keydown", handleKeyPress);
    clearTimeout(controlsTimeout);
    clearTimeout(indicatorTimeout);

    // Stop and cache the torrent stream when leaving video player
    if (handleId !== null) {
      try {
        await invoke("stop_stream", {
          handleId: handleId,
          deleteFiles: false // Cache the torrent instead of deleting
        });
        console.log("Torrent stream cached on component destroy");
      } catch (error) {
        console.error("Failed to stop stream on destroy:", error);
      }
    }

    // Cleanup DASH instance
    if (videoElement && videoElement.dashInstance) {
      videoElement.dashInstance.reset();
      videoElement.dashInstance = null;
    }

    if (demuxer) {
      demuxer.destroy();
      demuxer = null;
    }

    if (subtitleRenderer) {
      subtitleRenderer.dispose();
      subtitleRenderer = null;
    }

    if (audioPlayer) {
      // HTML5 Audio cleanup
      if (audioPlayer instanceof Audio) {
        audioPlayer.pause();
        audioPlayer.src = '';
      } else {
        audioPlayer.dispose();
      }
      audioPlayer = null;
    }

    if (srtRenderer) {
      srtRenderer.dispose();
      srtRenderer = null;
    }
  });
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="video-player"
  bind:this={videoContainer}
  on:mousemove={handleMouseMove}
  class:fullscreen
  class:hide-cursor={!showControls && playing}
>
  <!-- svelte-ignore a11y-media-has-caption -->
  <video
    bind:this={videoElement}
    {src}
    on:timeupdate={handleTimeUpdate}
    on:loadedmetadata={handleLoadedMetadata}
    on:seeking={handleSeekingEvent}
    on:seeked={handleCanPlayEvent}
    on:waiting={handleWaitingEvent}
    on:canplay={handleCanPlayEvent}
    on:play={() => { if (!showBufferingIndicator) playing = true; }}
    on:pause={() => { if (!showBufferingIndicator) playing = false; }}
    on:click={togglePlay}
  />

  {#if loading}
    <div class="loading-overlay">
      <div class="loading-content">
        <!-- Phase indicator -->
        <div class="loading-phases">
          <div class="loading-phase" class:active={loadingPhase === 'initializing'} class:complete={['buffering', 'metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}>
            <div class="phase-icon">
              {#if ['buffering', 'metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}
                <i class="ri-check-line"></i>
              {:else}
                <span>1</span>
              {/if}
            </div>
            <span class="phase-label">Initialize</span>
          </div>
          <div class="phase-connector" class:complete={['buffering', 'metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}></div>
          <div class="loading-phase" class:active={loadingPhase === 'buffering'} class:complete={['metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}>
            <div class="phase-icon">
              {#if ['metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}
                <i class="ri-check-line"></i>
              {:else}
                <span>2</span>
              {/if}
            </div>
            <span class="phase-label">Buffer</span>
          </div>
          <div class="phase-connector" class:complete={['metadata', 'transcoding', 'demuxing', 'ready'].includes(loadingPhase)}></div>
          <div class="loading-phase" class:active={loadingPhase === 'metadata'} class:complete={['transcoding', 'demuxing', 'ready'].includes(loadingPhase)}>
            <div class="phase-icon">
              {#if ['transcoding', 'demuxing', 'ready'].includes(loadingPhase)}
                <i class="ri-check-line"></i>
              {:else}
                <span>3</span>
              {/if}
            </div>
            <span class="phase-label">Metadata</span>
          </div>
          <div class="phase-connector" class:complete={['transcoding', 'demuxing', 'ready'].includes(loadingPhase)}></div>
          <div class="loading-phase" class:active={loadingPhase === 'transcoding'} class:complete={['demuxing', 'ready'].includes(loadingPhase)}>
            <div class="phase-icon">
              {#if ['demuxing', 'ready'].includes(loadingPhase)}
                <i class="ri-check-line"></i>
              {:else}
                <span>4</span>
              {/if}
            </div>
            <span class="phase-label">Transcode</span>
          </div>
          <div class="phase-connector" class:complete={['demuxing', 'ready'].includes(loadingPhase)}></div>
          <div class="loading-phase" class:active={loadingPhase === 'demuxing'} class:complete={loadingPhase === 'ready'}>
            <div class="phase-icon">
              {#if loadingPhase === 'ready'}
                <i class="ri-check-line"></i>
              {:else}
                <span>5</span>
              {/if}
            </div>
            <span class="phase-label">Prepare</span>
          </div>
        </div>

        <div class="loading-status">{loadingStatus.status}</div>
        
        <!-- Progress bar -->
        <div class="loading-progress">
          {#if loadingPhase === 'buffering' && loadingStatus.total > 0 && loadingStatus.peers === 0}
            <!-- Determinate progress bar for buffering with actual progress -->
            <div class="progress-bar-loading">
              <div 
                class="progress-fill"
                style="width: {(loadingStatus.progress / loadingStatus.total) * 100}%"
              ></div>
            </div>
            <div class="loading-stats">
              <span>{(loadingStatus.progress / 1024 / 1024).toFixed(1)} MB / {(loadingStatus.total / 1024 / 1024).toFixed(1)} MB</span>
              {#if loadingStatus.speed > 0}
                <span class="speed-stat">{(loadingStatus.speed / 1024 / 1024).toFixed(1)} MB/s</span>
              {/if}
            </div>
          {:else if loadingPhase === 'transcoding' && loadingStatus.transcodeProgress !== undefined}
            <!-- Determinate progress bar for transcoding -->
            <div class="progress-bar-loading">
              <div 
                class="progress-fill"
                style="width: {loadingStatus.transcodeProgress}%"
              ></div>
            </div>
            <div class="loading-stats">
              <span>{loadingStatus.transcodeProgress.toFixed(0)}% complete</span>
            </div>
          {:else}
            <!-- Indeterminate progress bar for other phases -->
            <div class="progress-bar-loading indeterminate">
              <div class="progress-fill-indeterminate"></div>
            </div>
          {/if}
        </div>
        
        <!-- Peer count during buffering -->
        {#if loadingPhase === 'buffering' && loadingStatus.peers > 0}
          <div class="loading-stats peer-stats">
            <span class="peer-stat">
              <i class="ri-group-line"></i>
              {loadingStatus.peers} peer{loadingStatus.peers !== 1 ? 's' : ''}
            </span>
          </div>
        {/if}
        
        <button class="cancel-loading-btn" on:click={close}>
          <i class="ri-close-line"></i>
          Cancel
        </button>
      </div>
    </div>
  {/if}

  <!-- Buffering indicator -->
  {#if showBufferingIndicator}
    <div class="buffering-indicator">
      <div class="buffering-spinner">
        <i class="ri-loader-4-line"></i>
      </div>
    </div>
  {/if}

  <div class="player-header" class:visible={showControls}>
    <button class="back-btn" on:click={close}>
      <i class="ri-arrow-left-line"></i>
    </button>
    <div class="player-title">{title}</div>
  </div>

  <!-- SubtitlesOctopus automatically creates and manages its own canvas as a sibling of the video element -->

  <!-- Keyboard shortcut indicator -->
  {#if showIndicator}
    <div class="shortcut-indicator {indicatorType}">
      {#key indicatorAnimationKey}
        <div class="indicator-content">
          {#key indicatorNudgeKey}
            <div class="indicator-icon">
              <i class="{indicatorIcon}"></i>
            </div>
          {/key}
          <div class="indicator-value">{indicatorValue}</div>
        </div>
      {/key}
    </div>
  {/if}

  <!-- Skip Section Button -->
  {#if showSkipPrompts && chapters && chapters.length > 0 && currentSkipSection && (skipTimerActive || showControls)}
    <button class="skip-button" on:click={skipSection}>
      <span class="skip-text">Skip {currentSkipSection.title}</span>
      <kbd class="skip-kbd">
        <i class="ri-corner-down-left-line"></i>
      </kbd>
      {#if skipTimerActive}
        {#key skipAnimationKey}
          <div class="skip-timer">
            <svg class="skip-timer-spinner" viewBox="0 0 20 20">
              <circle
                class="skip-timer-circle"
                cx="10"
                cy="10"
                r="8"
              />
            </svg>
          </div>
        {/key}
      {/if}
    </button>
  {/if}

  <!-- Next Episode Button -->
  {#if chapters && chapters.length > 0 && showNextEpisodeButton && seasonNum !== null && episodeNum !== null}
    <button class="skip-button next-episode" on:click={goToNextEpisode}>
      <span class="skip-text">Next Episode</span>
      <kbd class="skip-kbd">
        <i class="ri-corner-down-left-line"></i>
      </kbd>
    </button>
  {/if}

  <div class="controls" class:visible={showControls}>
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="progress-bar"
      bind:this={progressBar}
      on:mousedown={startDrag}
      on:mousemove={handleProgressHover}
      on:mouseleave={handleProgressLeave}
    >
      <div
        class="progress-buffered"
        style="width: {(buffered / duration) * 100}%"
      ></div>
      <div
        class="progress-filled"
        style="width: {((isSeeking ? seekPreviewTime : currentTime) /
          duration) *
          100}%; transition: {(isSeeking || justSeeked) ? 'none' : 'width 0.1s linear'}"
      >
        <div class="progress-handle"></div>
      </div>

      <!-- Chapter markers -->
      {#if chapters && chapters.length > 0}
        {#each chapters as chapter}
          {#if chapter.start_time > 0}
            <div
              class="chapter-marker"
              style="left: {(chapter.start_time / duration) * 100}%"
              title="{formatTime(chapter.start_time)} - {chapter.title ||
                `Chapter ${chapter.index + 1}`}"
            ></div>
          {/if}
        {/each}
      {/if}

      <!-- Hover preview tooltip -->
      {#if hoverTime !== null && !isSeeking}
        {@const hoverChapter = chapters
          .filter((ch) => ch.start_time <= hoverTime)
          .sort((a, b) => b.start_time - a.start_time)[0]}
        <div class="time-tooltip" style="left: {hoverX}px">
          <div class="tooltip-time">{formatTime(hoverTime)}</div>
          {#if hoverChapter}
            <div class="tooltip-chapter">
              {hoverChapter.title || `Chapter ${hoverChapter.index + 1}`}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Seeking preview tooltip -->
      {#if isSeeking}
        {@const seekChapterMatch = chapters
          .filter((ch) => ch.start_time <= seekPreviewTime)
          .sort((a, b) => b.start_time - a.start_time)[0]}
        <div
          class="time-tooltip"
          style="left: {(seekPreviewTime / duration) *
            progressBar?.getBoundingClientRect().width || 0}px"
        >
          <div class="tooltip-time">{formatTime(seekPreviewTime)}</div>
          {#if seekChapterMatch}
            <div class="tooltip-chapter">
              {seekChapterMatch.title ||
                `Chapter ${seekChapterMatch.index + 1}`}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="control-buttons">
      <button on:click={togglePlay} class="play-btn">
        <i class={playing ? "ri-pause-fill" : "ri-play-fill"}></i>
      </button>

      <span class="time"
        >{formatTime(currentTime)} / {formatTime(duration)}</span
      >

      <div class="spacer"></div>

      <div class="volume-control">
        <button on:click={toggleMute} class="volume-btn control-btn">
          {#if muted || volume === 0}
            <i class="ri-volume-mute-fill"></i>
          {:else if volume < 0.5}
            <i class="ri-volume-down-fill"></i>
          {:else}
            <i class="ri-volume-up-fill"></i>
          {/if}
        </button>
        <div class="volume-slider-wrapper">
          <div class="volume-slider-track">
            <div
              class="volume-slider-fill"
              style="height: {volume * 100}%"
            ></div>
            <div
              class="volume-slider-thumb"
              style="bottom: {volume * 100}%"
            ></div>
          </div>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={volume}
            on:input={changeVolume}
            orient="vertical"
            class="volume-slider-input"
          />
        </div>
      </div>

      {#if chapters && chapters.length > 0}
        <div class="player-track-menu-container">
          <button
            on:click={() => {
              showChaptersMenu = !showChaptersMenu;
              if (showChaptersMenu) {
                showAudioMenu = false;
                showSubtitleMenu = false;
                showPlayerMenu = false;
              }
            }}
            class="chapters-btn control-btn"
          >
            <i class="ri-list-check"></i>
          </button>
          {#if showChaptersMenu}
            <div class="player-track-dropdown chapters-menu">
              {#each chapters as chapter}
                <button
                  class="player-track-option"
                  on:click={() => jumpToChapter(chapter.start_time)}
                >
                  <span class="chapter-time"
                    >{formatTime(chapter.start_time)}</span
                  >
                  <span class="chapter-title"
                    >{chapter.title || `Chapter ${chapter.index + 1}`}</span
                  >
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="player-track-menu-container">
        <button
          on:click={togglePlayerMenu}
          class="menu-btn control-btn"
        >
          <i class="ri-settings-3-line"></i>
        </button>
        {#if showPlayerMenu}
          <div class="player-track-dropdown player-menu" bind:this={playerMenuElement}>
            <!-- Audio Submenu -->
            {#if videoMetadata?.audio_tracks && videoMetadata.audio_tracks.length > 0}
              <div class="submenu-container">
                <button
                  class="player-track-option menu-item submenu-trigger"
                  on:click={toggleAudioSubmenu}
                >
                  <span class="player-track-info">
                    <i class="ri-music-2-line"></i> Audio Track
                  </span>
                  <i class="ri-arrow-right-s-line"></i>
                </button>
              </div>
            {/if}

            <!-- Subtitle Submenu -->
            {#if (videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0) || externalSubtitles.length > 0}
              <div class="submenu-container">
                <button
                  class="player-track-option menu-item submenu-trigger"
                  on:click={toggleSubtitleSubmenu}
                >
                  <span class="player-track-info">
                    <i class="ri-closed-captioning-line"></i> Subtitles
                  </span>
                  <i class="ri-arrow-right-s-line"></i>
                </button>
              </div>
            {/if}

            <div class="menu-divider"></div>

            <!-- Actions -->
            <button
              class="player-track-option menu-item"
              on:click={openInExternalPlayer}
            >
              <span class="player-track-info">
                <i class="ri-external-link-line"></i> Open in external player
              </span>
            </button>
          </div>
        {/if}

        <!-- Audio Submenu (floating) -->
        {#if showAudioSubmenu && videoMetadata?.audio_tracks}
          <div class="submenu" style="left: {audioSubmenuX}px; top: {audioSubmenuY}px;" bind:this={audioSubmenuElement}>
            {#each videoMetadata.audio_tracks as track, i}
              <button
                class="player-track-option menu-item"
                class:active={selectedAudioTrack === i}
                disabled={loadingAudio}
                on:click={() => selectAudioTrack(i)}
              >
                <span class="player-track-info">
                  {#if track.language}
                    <span class="player-track-lang">{track.language.toUpperCase()}</span>
                  {:else}
                    Track {i + 1}
                  {/if}
                  {#if track.name}
                    <span class="player-track-detail">({track.name})</span>
                  {/if}
                </span>
                {#if loadingAudio && selectedAudioTrack === i}
                  <div class="loading-spinner-small"></div>
                {:else if track.codec}
                  <span class="player-track-badge">{track.codec}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

        <!-- Subtitle Submenu (floating) -->
        {#if showSubtitleSubmenu && ((videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0) || externalSubtitles.length > 0)}
          <div class="submenu" style="left: {subtitleSubmenuX}px; top: {subtitleSubmenuY}px;" bind:this={subtitleSubmenuElement}>
            <button
              class="player-track-option menu-item"
              class:active={selectedSubtitleTrack === -1}
              on:click={disableSubtitles}
            >
              <span class="player-track-info">Off</span>
            </button>
            {#if videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0}
              {#each videoMetadata.subtitle_tracks as track, i}
                <button
                  class="player-track-option menu-item"
                  class:active={selectedSubtitleTrack === i}
                  on:click={() => selectSubtitle(track, i)}
                  disabled={loadingSubtitle}
                >
                  <span class="player-track-info">
                    <span class="player-track-lang">{track.language ? track.language.toUpperCase() : `Subtitle ${i + 1}`}</span>
                    {#if track.name}
                      <span class="player-track-detail">{track.name}</span>
                    {/if}
                  </span>
                  <span class="player-track-badge">{track.codec || 'MKV'}</span>
                  {#if loadingSubtitle && selectedSubtitleTrack === i}
                    <span class="loading-spinner-small"></span>
                  {/if}
                </button>
              {/each}
            {/if}
            {#if externalSubtitles.length > 0}
              {#each externalSubtitles as track, i}
                <button
                  class="player-track-option menu-item"
                  class:active={selectedSubtitleTrack === `external-${i}`}
                  on:click={() => selectSubtitle(track, `external-${i}`)}
                  disabled={loadingSubtitle}
                >
                  <span class="player-track-info">
                    <span class="player-track-lang">{track.language ? track.language.toUpperCase() : `External ${i + 1}`}</span>
                  </span>
                  <span class="player-track-badge">SRT</span>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      <button on:click={toggleFullscreen} class="fullscreen-btn control-btn">
        <i class={fullscreen ? "ri-fullscreen-exit-line" : "ri-fullscreen-line"}
        ></i>
      </button>
    </div>
  </div>

  <!-- External Player Overlay -->
  {#if playingInExternal}
    <div class="external-player-overlay">
      <div class="external-player-content">
        <i class="ri-external-link-line external-icon"></i>
        <h2>Playing in External Player</h2>
        <p>Video is being played in your external media player</p>
        <div class="external-actions">
          <button class="btn-standard" on:click={restoreInternalPlayer}>
            <i class="ri-play-circle-line"></i> Restore Integrated Player
          </button>
          <button class="btn-standard" on:click={goToNextEpisodeMenu} disabled={!seasonNum || !episodeNum}>
            <i class="ri-skip-forward-line"></i> Next Episode
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- styles moved to src/styles/main.css -->
