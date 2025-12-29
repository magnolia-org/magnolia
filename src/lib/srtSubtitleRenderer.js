export class SRTSubtitleRenderer {
  constructor(videoElement) {
    this.videoElement = videoElement;
    this.subtitles = [];
    this.currentCue = null;
    this.container = null;
    this.isVisible = true;
    this.streamingFetcher = null; // For streaming SRT subtitles from Wyzie
    this.httpPort = null; // HTTP port for backend streaming
    this.sessionId = null;
    this.fileId = null;
    this.trackIndex = null;
    this.cachedWindow = { start: 0, end: 0, entries: [] };
    this.isLoadingWindow = false;
    this.demuxer = null; // MKV demuxer for streaming subtitle extraction
    this.demuxerTrackIndex = null;
    this.videoDuration = null;
    this.fetchInterval = null;
    this.seekListener = null;
    this.lastFetchTime = 0;
    this.lastPlayheadTime = 0;
    this.offset = 0;
    this.styles = {
      fontSize: 24,
      backgroundOpacity: 0.5,
      color: '#ffffff',
      textShadow: true,
      textShadowColor: '#000000',
      windowMargin: 60
    };
  }

  initialize() {
    // Create subtitle container overlay
    this.container = document.createElement('div');
    this.container.className = 'srt-subtitle-overlay';
    this.container.style.cssText = `
      position: absolute;
      bottom: ${this.styles.windowMargin}px;
      left: 0;
      right: 0;
      text-align: center;
      pointer-events: none;
      z-index: 10;
      padding: 0 20px;
      transition: bottom 0.3s ease;
    `;
    
    const playerContainer = this.videoElement.closest('.video-player');
    if (playerContainer) {
      playerContainer.appendChild(this.container);
    }

    // Update subtitles on timeupdate
    this.videoElement.addEventListener('timeupdate', () => this.updateSubtitles());
  }

  setStyles(styles) {
    this.styles = { ...this.styles, ...styles };
    if (this.container) {
      this.container.style.bottom = `${this.styles.windowMargin}px`;
      // Re-render current subtitle if visible
      if (this.currentCue) {
        this.displaySubtitle(this.currentCue.text);
      }
    }
  }

  setOffset(offset) {
    this.offset = offset;
    this.updateSubtitles();
  }

  /**
   * Set streaming fetcher for dynamic subtitle loading (Wyzie API)
   */
  setStreamingFetcher(fetcher) {
    this.streamingFetcher = fetcher;
    console.log('[SRTRenderer] Streaming fetcher set');
  }

  /**
   * Enable HTTP streaming mode for MKV embedded SRT subtitles (uses ffmpeg extraction)
   */
  setHttpStreaming(httpPort, sessionId, fileId, trackIndex) {
    this.httpPort = httpPort;
    this.sessionId = sessionId;
    this.fileId = fileId;
    this.trackIndex = trackIndex;
    this.cachedWindow = { start: 0, end: 0, entries: [] };
    console.log('[SRTRenderer] HTTP streaming enabled:', { httpPort, sessionId, fileId, trackIndex });
  }

  /**
   * Enable demuxer streaming mode for MKV embedded SRT subtitles (uses web-demuxer)
   * Periodically fetches subtitle packets around playhead position
   */
  setDemuxerStreaming(demuxer, trackIndex, duration) {
    this.demuxer = demuxer;
    this.demuxerTrackIndex = trackIndex;
    this.videoDuration = duration;
    this.cachedWindow = { start: 0, end: 0, entries: [] };
    this.lastFetchTime = 0;
    this.lastPlayheadTime = 0;
    
    console.log('[SRTRenderer] Demuxer streaming enabled:', { trackIndex, duration });
    
    // Start periodic fetching
    this.startDemuxerFetching();
    
    // Listen for seeks
    this.setupSeekDetection();
  }

  /**
   * Setup seek detection to fetch immediately on seeks
   */
  setupSeekDetection() {
    if (this.seekListener) {
      this.videoElement.removeEventListener('seeked', this.seekListener);
    }
    
    this.seekListener = () => {
      console.log('[SRTRenderer] Seek detected, fetching subtitles immediately');
      this.fetchAroundPlayhead(true);
    };
    
    this.videoElement.addEventListener('seeked', this.seekListener);
  }

  /**
   * Start periodic fetching of subtitle packets around playhead
   */
  startDemuxerFetching() {
    if (this.fetchInterval) {
      clearInterval(this.fetchInterval);
    }
    
    // Fetch immediately and then every 3 seconds
    this.fetchAroundPlayhead();
    this.fetchInterval = setInterval(() => {
      this.fetchAroundPlayhead();
    }, 3000);
  }

  /**
   * Fetch subtitle packets around current playhead position
   */
  async fetchAroundPlayhead(forceFetch = false) {
    if (!this.demuxer || !this.isVisible) return;
    
    const currentTime = this.videoElement.currentTime;
    const now = Date.now();
    
    // Detect seek (jump > 2 seconds)
    const timeDiff = Math.abs(currentTime - this.lastPlayheadTime);
    const isSeeking = timeDiff > 2;
    this.lastPlayheadTime = currentTime;
    
    // Debounce: don't fetch if we just fetched less than 500ms ago (unless forced or seeking)
    if (!forceFetch && !isSeeking && (now - this.lastFetchTime) < 500) {
      return;
    }
    
    const windowStart = Math.max(0, currentTime - 60); // 60s before (larger window)
    const windowEnd = Math.min(this.videoDuration, currentTime + 120); // 120s after
    
    // Only skip if we have a very generous cached window and not seeking
    if (!forceFetch && !isSeeking && 
        windowStart >= this.cachedWindow.start - 10 && 
        windowEnd <= this.cachedWindow.end + 10 &&
        this.cachedWindow.entries.length > 0) {
      return;
    }
    
    this.lastFetchTime = now;
    console.log('[SRTRenderer] Fetching subtitles from demuxer:', { windowStart, windowEnd, currentTime, isSeeking, forceFetch });
    
    try {
      const packets = await this.demuxer.readSubtitlePacketsInWindow(
        this.demuxerTrackIndex,
        windowStart,
        windowEnd
      );
      
      if (packets && packets.length > 0) {
        console.log('[SRTRenderer] Received', packets.length, 'subtitle packets');
        
        // Merge with existing cached entries
        const mergedEntries = [...this.cachedWindow.entries];
        
        for (const packet of packets) {
          // Check if we already have this entry
          const exists = mergedEntries.some(
            e => Math.abs(e.start - packet.start) < 0.1 && e.text === packet.text
          );
          
          if (!exists) {
            mergedEntries.push(packet);
          }
        }
        
        // Sort by start time
        mergedEntries.sort((a, b) => a.start - b.start);
        
        // Update cached window
        this.cachedWindow = {
          start: Math.min(this.cachedWindow.start, windowStart),
          end: Math.max(this.cachedWindow.end, windowEnd),
          entries: mergedEntries
        };
        
        // Update subtitles array
        this.subtitles = mergedEntries;
        
        console.log('[SRTRenderer] Total cached entries:', mergedEntries.length);
      }
    } catch (error) {
      console.error('[SRTRenderer] Error fetching from demuxer:', error);
    }
  }

  /**
   * Set subtitles directly (used with streaming fetcher)
   */
  setSubtitles(subtitles) {
    this.subtitles = subtitles;
    console.log('[SRTRenderer] Set subtitles:', subtitles.length);
  }

  parseSRT(srtText) {
    const lines = srtText.trim().split('\n');
    const subtitles = [];
    let current = {};
    
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      
      // Skip subtitle index numbers
      if (/^\d+$/.test(line)) {
        current = {};
        continue;
      }
      
      // Parse timestamp line
      if (line.includes('-->')) {
        const [start, end] = line.split('-->').map(t => this.parseTimestamp(t.trim()));
        current.start = start;
        current.end = end;
        current.text = '';
        continue;
      }
      
      // Parse subtitle text
      if (line && current.start !== undefined) {
        current.text += (current.text ? '\n' : '') + line;
      }
      
      // Empty line marks end of subtitle block
      if (!line && current.start !== undefined) {
        subtitles.push(current);
        current = {};
      }
    }
    
    // Add last subtitle if exists
    if (current.start !== undefined) {
      subtitles.push(current);
    }
    
    this.subtitles = subtitles;
    console.log('[SRTRenderer] Parsed SRT subtitles:', this.subtitles.length);
  }

  parseTimestamp(timeStr) {
    // Parse timestamp like "00:00:20,000" or "00:00:20.000"
    const parts = timeStr.replace(',', '.').split(':');
    const hours = parseInt(parts[0]);
    const minutes = parseInt(parts[1]);
    const secondsParts = parts[2].split('.');
    const seconds = parseInt(secondsParts[0]);
    const milliseconds = parseInt(secondsParts[1] || 0);
    
    return hours * 3600 + minutes * 60 + seconds + milliseconds / 1000;
  }

  async updateSubtitles() {
    if (!this.isVisible || !this.container) {
      console.log('[SRTRenderer] Update skipped - visible:', this.isVisible, 'container:', !!this.container);
      return;
    }
    
    const currentTime = this.videoElement.currentTime;
    const adjustedTime = currentTime - this.offset;
    
    // Use HTTP streaming if available
    if (this.httpPort && this.sessionId !== null && this.fileId !== null && this.trackIndex !== null) {
      await this.updateFromHttpStream(adjustedTime);
      return;
    }
    
    // Use streaming fetcher if available, otherwise use cached subtitles
    const subtitles = this.streamingFetcher 
      ? this.streamingFetcher.getSubtitles()
      : this.subtitles;
    
    if (subtitles.length === 0) {
      console.log('[SRTRenderer] No subtitles available');
      return;
    }
    
    const activeSub = subtitles.find(
      sub => adjustedTime >= sub.start && adjustedTime <= sub.end
    );
    
    if (activeSub && activeSub !== this.currentCue) {
      console.log('[SRTRenderer] Showing subtitle at', adjustedTime.toFixed(2), ':', activeSub.text);
      this.currentCue = activeSub;
      this.displaySubtitle(activeSub.text);
    } else if (!activeSub && this.currentCue) {
      console.log('[SRTRenderer] Hiding subtitle at', adjustedTime.toFixed(2));
      this.currentCue = null;
      this.hideSubtitle();
    }
  }

  async updateFromHttpStream(currentTime) {
    // Check if we need to load a new window
    const needsNewWindow = currentTime < this.cachedWindow.start || 
                          currentTime > this.cachedWindow.end;
    
    if (needsNewWindow && !this.isLoadingWindow) {
      this.isLoadingWindow = true;
      
      // Load a 60-second window around current time
      const windowStart = Math.max(0, currentTime - 10);
      const windowEnd = currentTime + 50;
      
      try {
        const url = `http://localhost:${this.httpPort}/torrents/${this.sessionId}/srt-stream/${this.fileId}/${this.trackIndex}`;
        
        const response = await fetch(url, {
          headers: {
            'X-Subtitle-Start': windowStart.toString(),
            'X-Subtitle-End': windowEnd.toString()
          }
        });
        
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        
        const srtText = await response.text();
        const entries = this.parseSRTToEntries(srtText);
        
        this.cachedWindow = {
          start: windowStart,
          end: windowEnd,
          entries: entries
        };
        
        console.log(`[SRTRenderer] Loaded ${entries.length} subtitle entries for window ${windowStart.toFixed(1)}s-${windowEnd.toFixed(1)}s`);
      } catch (error) {
        console.error('[SRTRenderer] Failed to load subtitle window from HTTP:', error);
      } finally {
        this.isLoadingWindow = false;
      }
    }
    
    // Find active subtitle in cached window
    const activeSub = this.cachedWindow.entries.find(
      sub => currentTime >= sub.start && currentTime <= sub.end
    );
    
    if (activeSub && activeSub !== this.currentCue) {
      this.currentCue = activeSub;
      this.displaySubtitle(activeSub.text);
    } else if (!activeSub && this.currentCue) {
      this.currentCue = null;
      this.hideSubtitle();
    }
  }

  parseSRTToEntries(srtText) {
    const lines = srtText.trim().split('\n');
    const entries = [];
    let current = {};
    
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      
      // Skip subtitle index numbers
      if (/^\d+$/.test(line)) {
        current = {};
        continue;
      }
      
      // Parse timestamp line
      if (line.includes('-->')) {
        const [start, end] = line.split('-->').map(t => this.parseTimestamp(t.trim()));
        current.start = start;
        current.end = end;
        current.text = '';
        continue;
      }
      
      // Parse subtitle text
      if (line && current.start !== undefined) {
        current.text += (current.text ? '\n' : '') + line;
      }
      
      // Empty line marks end of subtitle block
      if (!line && current.start !== undefined) {
        entries.push(current);
        current = {};
      }
    }
    
    // Add last subtitle if exists
    if (current.start !== undefined) {
      entries.push(current);
    }
    
    return entries;
  }

  async updateFromDemuxer(currentTime) {
    // Check if we need to load a new window
    const needsNewWindow = currentTime < this.cachedWindow.start || 
                          currentTime > this.cachedWindow.end;
    
    if (needsNewWindow && !this.isLoadingWindow) {
      this.isLoadingWindow = true;
      
      // Load a 60-second window around current time
      const windowStart = Math.max(0, currentTime - 10);
      const windowEnd = currentTime + 50;
      
      try {
        const entries = await this.demuxer.readSubtitlePacketsInWindow(
          this.trackIndex,
          windowStart,
          windowEnd
        );
        
        this.cachedWindow = {
          start: windowStart,
          end: windowEnd,
          entries: entries
        };
        
        console.log(`[SRTRenderer] Loaded ${entries.length} subtitle packets for window ${windowStart.toFixed(1)}s-${windowEnd.toFixed(1)}s`);
      } catch (error) {
        console.error('[SRTRenderer] Failed to load subtitle window:', error);
      } finally {
        this.isLoadingWindow = false;
      }
    }
    
    // Find active subtitle in cached window
    const activeSub = this.cachedWindow.entries.find(
      sub => currentTime >= sub.start && currentTime <= sub.end
    );
    
    if (activeSub && activeSub !== this.currentCue) {
      this.currentCue = activeSub;
      this.displaySubtitle(activeSub.text);
    } else if (!activeSub && this.currentCue) {
      this.currentCue = null;
      this.hideSubtitle();
    }
  }

  displaySubtitle(text) {
    if (!this.container) return;
    
    // Parse position tags
    let positionStyle = '';
    const posMatch = text.match(/X1:(\d+)\s*X2:(\d+)\s*Y1:(\d+)\s*Y2:(\d+)/);
    if (posMatch) {
      const [, x1, x2, y1, y2] = posMatch.map(Number);
      // Remove position tags from text
      text = text.replace(/X1:\d+\s*X2:\d+\s*Y1:\d+\s*Y2:\d+/g, '').trim();
      
      // Calculate position (assuming video is 100% width/height)
      const leftPercent = (x1 / 1920) * 100; // Assuming 1920x1080 reference
      const topPercent = (y1 / 1080) * 100;
      positionStyle = `left: ${leftPercent}%; top: ${topPercent}%; bottom: auto; transform: none;`;
    }
    
    // Convert newlines to <br> and format SRT tags
    const htmlText = text
      .split('\n')
      .map(line => this.formatSRT(line))
      .join('<br>');
    
    this.container.innerHTML = `
      <div style="
        display: inline-block;
        background: rgba(0, 0, 0, ${this.styles.backgroundOpacity});
        padding: 8px 16px;
        border-radius: 4px;
        font-size: ${this.styles.fontSize}px;
        font-weight: 500;
        color: ${this.styles.color};
        text-shadow: ${this.styles.textShadow ? `2px 2px 2px ${this.styles.textShadowColor}` : 'none'};
        line-height: 1.4;
        max-width: 80%;
        ${positionStyle}
      ">
        ${htmlText}
      </div>
    `;
  }

  hideSubtitle() {
    if (this.container) {
      this.container.innerHTML = '';
    }
  }

  formatSRT(text) {
    // Position tags (X1, Y1, X2, Y2) are handled in displaySubtitle
    
    // Convert {i} to <i> (alternate italic format)
    text = text.replace(/\{i\}/g, '<i>').replace(/\{\/i\}/g, '</i>');
    text = text.replace(/\{b\}/g, '<b>').replace(/\{\/b\}/g, '</b>');
    text = text.replace(/\{u\}/g, '<u>').replace(/\{\/u\}/g, '</u>');
    
    // Support <font color="..."> tags
    // Handles: color="#hex", color="name", color=#hex, color=name
    text = text.replace(/<font\s+color=["']?((?:#[0-9a-fA-F]{3,6})|[a-zA-Z]+)["']?>/gi, (match, color) => {
      return `<span style="color: ${color}">`;
    });
    text = text.replace(/<\/font>/gi, '</span>');
    
    // Ensure standard tags are preserved: <i>, <b>, <u>, <span>
    // These are already HTML tags, so they'll work as-is
    
    // Escape other potentially dangerous characters while preserving our formatting tags
    const allowedTags = /<\/?(?:i|b|u|span[^>]*)>/gi;
    const parts = text.split(allowedTags);
    const tags = text.match(allowedTags) || [];
    
    // Escape text parts but not tags
    let result = '';
    for (let i = 0; i < parts.length; i++) {
      if (parts[i]) {
        // Escape the text
        result += parts[i]
          .replace(/&/g, '&amp;')
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;');
      }
      if (tags[i]) {
        // Add the tag as-is
        result += tags[i];
      }
    }
    
    return result;
  }

  show() {
    this.isVisible = true;
    if (!this.container) {
      this.initialize();
    }
    if (this.container) {
      this.container.style.display = 'block';
      // Force update to show current subtitle if any
      this.updateSubtitles();
    }
    // Restart demuxer fetching when showing again
    if (this.demuxer && !this.fetchInterval) {
      this.startDemuxerFetching();
      this.setupSeekDetection();
    }
  }

  hide() {
    this.isVisible = false;
    this.hideSubtitle();
    if (this.container) {
      this.container.style.display = 'none';
    }
    // Stop demuxer fetching when hiding
    if (this.fetchInterval) {
      clearInterval(this.fetchInterval);
      this.fetchInterval = null;
    }
    // Remove seek listener
    if (this.seekListener) {
      this.videoElement.removeEventListener('seeked', this.seekListener);
      this.seekListener = null;
    }
  }

  clearCache() {
    // Clear cached subtitle window (useful on seek)
    this.cachedWindow = { start: 0, end: 0, entries: [] };
    this.currentCue = null;
    this.hideSubtitle();
  }

  dispose() {
    // Clean up demuxer interval
    if (this.fetchInterval) {
      clearInterval(this.fetchInterval);
      this.fetchInterval = null;
    }
    
    // Remove seek listener
    if (this.seekListener) {
      this.videoElement.removeEventListener('seeked', this.seekListener);
      this.seekListener = null;
    }
    
    if (this.container && this.container.parentNode) {
      this.container.parentNode.removeChild(this.container);
    }
    this.subtitles = [];
    this.currentCue = null;
    this.container = null;
    this.streamingFetcher = null;
    this.httpPort = null;
    this.sessionId = null;
    this.fileId = null;
    this.trackIndex = null;
    this.cachedWindow = { start: 0, end: 0, entries: [] };
    this.demuxer = null;
    this.demuxerTrackIndex = null;
    this.videoDuration = null;
  }
}
