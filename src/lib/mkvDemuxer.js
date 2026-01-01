import { WebDemuxer } from 'web-demuxer';
import { invoke } from '@tauri-apps/api/core';

export class MKVDemuxer {
  constructor() {
    this.demuxer = null;
    this.mediaInfo = null;
    this.videoTrack = null;
    this.audioTracks = [];
    this.subtitleTracks = [];
    this.chapters = [];
    this.attachments = [];
    this.onReady = null;
    this.onVideoSamples = null;
    this.onAudioSamples = null;
    this.onSubtitleData = null;
    this.videoStream = null;
    this.audioStreams = new Map();
    this.audioReaders = new Map();
    this.cancelSignals = new Map();
  }

  async initialize(src) {
    try {
      const wasmUrl = new URL('/web-demuxer.wasm', window.location.origin).href;
      console.log('WASM URL:', wasmUrl);

      this.demuxer = new WebDemuxer({
        wasmFilePath: wasmUrl
      });

      await this.demuxer.load(src);

      this.mediaInfo = await this.demuxer.getMediaInfo();
      console.log('Web-Demuxer Media Info:', this.mediaInfo);

      await this.processMediaInfo();

      const info = {
        duration: this.mediaInfo.duration,
        videoTrack: this.videoTrack,
        audioTracks: this.audioTracks,
        subtitleTracks: this.subtitleTracks,
        chapters: this.chapters
      };

      if (this.onReady) {
        this.onReady(info);
      }

      return info;
    } catch (error) {
      console.error('Failed to initialize web-demuxer:', error);
      throw error;
    }
  }

  async processMediaInfo() {
    const streams = this.mediaInfo.streams || [];
    
    console.log('Processing streams:', streams.length, 'total streams');
    
    for (const stream of streams) {
      const codecType = stream.codec_type_string || stream.codecTypeString;
      
      if (codecType === 'video' && !this.videoTrack) {
        this.videoTrack = {
          id: stream.index,
          codec: stream.codec_name || stream.codecName,
          width: stream.width,
          height: stream.height,
          duration: this.mediaInfo.duration
        };
        console.log('Found video track:', this.videoTrack);
      } else if (codecType === 'audio') {
        const audioTrack = {
          id: stream.index,
          codec: stream.codec_name || stream.codecName,
          sampleRate: stream.sample_rate || stream.sampleRate || 48000,
          channels: stream.channels || 2,
          language: stream.tags?.language || 'und',
          name: stream.tags?.title || `Audio Track ${this.audioTracks.length + 1}`,
          extradata: stream.extradata,
          rawStream: stream
        };
        this.audioTracks.push(audioTrack);
        console.log('Found audio track:', audioTrack);
        if (audioTrack.extradata) {
          console.log(`Audio track ${audioTrack.id} has extradata:`, audioTrack.extradata.byteLength || audioTrack.extradata.length, 'bytes');
        } else {
          console.log(`Audio track ${audioTrack.id} has NO extradata`);
        }
      } else if (codecType === 'subtitle') {
        const subtitleTrack = {
          id: stream.index,
          codec: stream.codec_name || stream.codecName,
          language: stream.tags?.language || 'und',
          name: stream.tags?.title || `Subtitle Track ${this.subtitleTracks.length + 1}`,
          extradata: stream.extradata,
          rawStream: stream
        };
        this.subtitleTracks.push(subtitleTrack);
        console.log('Found subtitle track:', subtitleTrack);
      } else if (codecType === 'attachment') {
        const filename = stream.tags?.filename || stream.filename;
        const mimetype = stream.tags?.mimetype || stream.mimetype;
        
        const isFontFile = mimetype?.startsWith('font/') || 
                          mimetype?.includes('ttf') ||
                          mimetype?.includes('otf') ||
                          filename?.match(/\.(ttf|otf|woff|woff2)$/i);
        
        if (isFontFile) {
          const attachment = {
            id: stream.index,
            filename: filename || `attachment_${this.attachments.length}`,
            mimeType: mimetype || 'application/octet-stream',
            data: stream.extradata,
            rawAttachment: stream
          };
          this.attachments.push(attachment);
          console.log('Found font attachment:', attachment.filename, mimetype);
        }
      }
    }
    
    // Also check legacy structure for compatibility
    if (this.audioTracks.length === 0 && this.mediaInfo.audioStreams) {
      this.audioTracks = this.mediaInfo.audioStreams.map((stream, index) => ({
        id: stream.index || index,
        codec: stream.codec_name || stream.codecName,
        sampleRate: stream.sample_rate || stream.sampleRate || 48000,
        channels: stream.channels || 2,
        language: stream.tags?.language || stream.metadata?.language || 'und',
        name: stream.tags?.title || stream.metadata?.title || `Audio Track ${index + 1}`,
        rawStream: stream
      }));
    }
    
    if (this.subtitleTracks.length === 0 && this.mediaInfo.subtitleStreams) {
      this.subtitleTracks = this.mediaInfo.subtitleStreams.map((stream, index) => ({
        id: stream.index || index,
        codec: stream.codec_name || stream.codecName,
        language: stream.tags?.language || stream.metadata?.language || 'und',
        name: stream.tags?.title || stream.metadata?.title || `Subtitle Track ${index + 1}`,
        extradata: stream.extradata,
        rawStream: stream
      }));
    }

    if (this.mediaInfo.chapters && this.mediaInfo.chapters.length > 0) {
      this.chapters = this.mediaInfo.chapters.map((chapter, index) => ({
        index,
        start_time: chapter.startTime || chapter.start_time,
        end_time: chapter.endTime || chapter.end_time,
        title: chapter.metadata?.title || chapter.tags?.title || `Chapter ${index + 1}`
      }));
    }
    
    console.log('Processed tracks:', {
      video: this.videoTrack ? 1 : 0,
      audio: this.audioTracks.length,
      subtitles: this.subtitleTracks.length,
      fonts: this.attachments.length,
      chapters: this.chapters.length
    });
  }

  async startExtracting(selectedAudioTrackId = 0) {
    if (!this.demuxer) return;

    if (this.videoTrack) {
      this.videoStream = this.demuxer.read('video', 0, this.videoTrack.id);
      this.readVideoStream();
    }

    if (this.audioTracks.length > 0 && selectedAudioTrackId < this.audioTracks.length) {
      const track = this.audioTracks[selectedAudioTrackId];
      console.log(`Starting audio extraction for track ${selectedAudioTrackId}, stream index ${track.id}`);
      console.log('Track details:', track);
      
      try {
        const stream = this.demuxer.read('audio', 0, track.id);
        console.log('Audio stream created:', !!stream, 'type:', typeof stream);
        
        if (!stream) {
          console.error('demuxer.read() returned null/undefined for audio');
          return;
        }
        
        this.audioStreams.set(track.id, stream);
        this.readAudioStream(track.id, stream);
      } catch (error) {
        console.error('Failed to create audio stream:', error);
      }
    }
  }

  async readVideoStream() {
    if (!this.videoStream) return;

    try {
      const reader = this.videoStream.getReader();

      while (true) {
        const { done, value } = await reader.read();

        if (done) break;

        if (this.onVideoSamples) {
          this.onVideoSamples([value]);
        }
      }
    } catch (error) {
      console.error('Error reading video stream:', error);
    }
  }

  async readAudioStream(trackId, stream) {
    if (!stream) return;

    let reader = null;
    try {
      reader = stream.getReader();
      this.audioReaders.set(trackId, reader);
      this.cancelSignals.set(trackId, false);

      console.log(`Starting to read audio stream for track ${trackId}`);
      
      while (true) {
        // Check if we should cancel
        if (this.cancelSignals.get(trackId)) {
          console.log(`Audio stream ${trackId} canceled`);
          break;
        }

        const { done, value } = await reader.read();

        if (done) {
          console.log(`Audio stream ${trackId} done reading`);
          break;
        }

        console.log(`Read audio sample from track ${trackId}:`, {
          hasValue: !!value,
          hasData: !!value?.data,
          dataSize: value?.data?.byteLength,
          timestamp: value?.timestamp
        });

        if (this.onAudioSamples && !this.cancelSignals.get(trackId)) {
          this.onAudioSamples(trackId, [value]);
        } else {
          console.warn(`Cannot deliver audio sample: onAudioSamples=${!!this.onAudioSamples}, canceled=${this.cancelSignals.get(trackId)}`);
        }
      }
    } catch (error) {
      // Only log if it's not a cancellation error
      if (error.name !== 'AbortError' && !error.message?.includes('cancel')) {
        console.error('Error reading audio stream:', {
          error,
          name: error?.name,
          message: error?.message,
          stack: error?.stack,
          type: typeof error,
          string: String(error)
        });
      }
    } finally {
      // Always release the lock and clean up
      if (reader) {
        try {
          reader.releaseLock();
        } catch (e) {
          // Lock already released
        }
        this.audioReaders.delete(trackId);
      }
      this.cancelSignals.delete(trackId);
    }
  }

  stop() {
    if (this.videoStream) {
      try {
        this.videoStream.cancel();
      } catch (e) {
        console.warn('Error canceling video stream:', e);
      }
      this.videoStream = null;
    }

    // Signal all audio streams to cancel
    this.cancelSignals.forEach((_, trackId) => {
      this.cancelSignals.set(trackId, true);
    });

    // Release all reader locks first
    this.audioReaders.forEach((reader, trackId) => {
      try {
        reader.releaseLock();
      } catch (e) {
        // Lock already released or reader closed
      }
    });
    this.audioReaders.clear();

    // Now cancel the streams
    this.audioStreams.forEach((stream, trackId) => {
      try {
        stream.cancel();
      } catch (e) {
        // Stream already canceled or closed
      }
    });
    this.audioStreams.clear();
    this.cancelSignals.clear();
  }

  async seek(time, selectedAudioTrackId = 0) {
    if (!this.demuxer) return;

    this.stop();

    if (this.videoTrack) {
      this.videoStream = this.demuxer.read('video', time, this.videoTrack.id);
      this.readVideoStream();
    }

    // Only seek the selected audio track using its stream index
    if (this.audioTracks.length > 0 && selectedAudioTrackId < this.audioTracks.length) {
      const track = this.audioTracks[selectedAudioTrackId];
      console.log(`Seeking audio track ${selectedAudioTrackId}, stream index ${track.id} to time ${time}`);
      const stream = this.demuxer.read('audio', time, track.id);
      this.audioStreams.set(track.id, stream);
      this.readAudioStream(track.id, stream);
    }
  }

  async switchAudioTrack(trackId, currentTime = 0) {
    if (!this.demuxer) return;

    // Signal cancellation for all current audio streams
    this.cancelSignals.forEach((_, trackId) => {
      this.cancelSignals.set(trackId, true);
    });

    // Release reader locks
    this.audioReaders.forEach((reader, trackId) => {
      try {
        reader.releaseLock();
      } catch (e) {
        // Lock already released
      }
    });
    this.audioReaders.clear();

    // Wait a bit for readers to finish
    await new Promise(resolve => setTimeout(resolve, 50));

    // Now cancel the streams
    this.audioStreams.forEach((stream, trackId) => {
      try {
        stream.cancel();
      } catch (e) {
        // Stream already canceled
      }
    });
    this.audioStreams.clear();
    this.cancelSignals.clear();

    // Start new audio track from current time
    const track = this.audioTracks.find(t => t.id === trackId);
    if (track) {
      console.log(`Switching to audio track with stream index ${track.id} at time ${currentTime}`);
      const stream = this.demuxer.read('audio', currentTime, track.id);
      this.audioStreams.set(track.id, stream);
      this.readAudioStream(track.id, stream);
    }
  }

  /**
   * Read subtitle packets in a time window for streaming playback
   */
  async readSubtitlePacketsInWindow(trackIndex, startTime, endTime) {
    if (!this.subtitleTracks[trackIndex] || !this.demuxer) return [];

    const track = this.subtitleTracks[trackIndex];
    const streamIndex = track.rawStream?.index ?? track.id;
    const decoder = new TextDecoder('utf-8');
    const subtitleEntries = [];

    try {
      const stream = this.demuxer.readAVPacket(
        Math.max(0, startTime - 5), // Start 5s before for context
        endTime + 5, // End 5s after
        3, // AVMediaType.AVMEDIA_TYPE_SUBTITLE
        streamIndex,
        1 // seek flag
      );
      const reader = stream.getReader();

      try {
        let packetCount = 0;
        const maxPackets = 1000; // Reasonable limit for a time window

        while (packetCount < maxPackets) {
          const { done, value } = await reader.read();
          if (done) break;
          packetCount++;

          if (value && value.data && value.data.byteLength > 0) {
            const text = decoder.decode(value.data).trim();
            if (text) {
              subtitleEntries.push({
                start: value.timestamp || 0,
                end: (value.timestamp || 0) + (value.duration || 0),
                text: text
              });
            }
          }
        }
      } finally {
        try {
          reader.releaseLock();
        } catch (e) {
          // Ignore
        }
      }

      return subtitleEntries;
    } catch (error) {
      console.error('Error reading subtitle window:', error);
      return [];
    }
  }

  async extractSubtitleTrack(trackIndex) {
    if (!this.subtitleTracks[trackIndex]) return null;

    try {
      const track = this.subtitleTracks[trackIndex];
      console.log(`Extracting subtitle track ${trackIndex}, stream index ${track.id}, codec: ${track.codec}`);
      
      // For ASS/SSA subtitles, the extradata contains the header and styles
      if (track.extradata && (track.codec === 'ass' || track.codec === 'ssa')) {
        console.log('Extracting ASS subtitle from extradata, size:', track.extradata.byteLength || track.extradata.length);
        
        // Decode the extradata which contains the ASS header
        const decoder = new TextDecoder('utf-8');
        let assHeader = decoder.decode(track.extradata);
        console.log('ASS Header preview:', assHeader.substring(0, 200));
        
        // Remove auto-generated comments that may not match original file
        // Keep only the essential ASS structure
        const lines = assHeader.split('\n');
        const filteredLines = lines.filter(line => {
          // Keep section headers, format lines, style definitions, but skip comments starting with ;
          const trimmed = line.trim();
          return !trimmed.startsWith(';') || trimmed === '';
        });
        assHeader = filteredLines.join('\n');
        
        // Ensure header ends with newline
        if (!assHeader.endsWith('\n')) {
          assHeader += '\n';
        }
        
        // If header doesn't contain [Events] section, we need to add it
        if (!assHeader.includes('[Events]')) {
          assHeader += '\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n';
        }
        
        // Now read the subtitle packets for the dialogue lines
        if (!this.demuxer) {
          console.log('No demuxer available, returning header only');
          return assHeader;
        }
        
        try {
          // Use readMediaPacket for subtitle streams (not read() which only supports video/audio)
          console.log('Reading subtitle stream with demuxer.readMediaPacket()...');
          console.log('Track index:', trackIndex, 'Track ID:', track.id, 'Stream index:', track.rawStream?.index);
          
          // Need to use the actual stream index from the raw stream data
          const streamIndex = track.rawStream?.index ?? track.id;
          
          // readAVPacket signature: (start, end, streamType, streamIndex, seekFlag)
          // AVMediaType: AVMEDIA_TYPE_SUBTITLE = 3
          const stream = this.demuxer.readAVPacket(
            0, // start time
            this.mediaInfo.duration || 0, // end time
            3, // AVMediaType.AVMEDIA_TYPE_SUBTITLE
            streamIndex, // stream index to read only this specific subtitle track
            1 // seek flag (backward)
          );
          const reader = stream.getReader();
          let dialogues = '';
          let packetCount = 0;
          let totalBytes = 0;

          try {
            // Set a reasonable limit to prevent infinite loops
            const maxPackets = 100000;
            
            while (packetCount < maxPackets) {
              const { done, value } = await reader.read();
              if (done) {
                console.log('Stream reading complete');
                break;
              }

              packetCount++;
              const dataSize = value.data?.byteLength || 0;
              totalBytes += dataSize;
              
              if (packetCount <= 5 || packetCount % 100 === 0) {
                console.log(`Subtitle packet ${packetCount}:`, {
                  timestamp: value.timestamp,
                  duration: value.duration,
                  size: dataSize,
                  keyframe: value.keyframe
                });
              }
              
              // ASS subtitle packets in MKV contain dialogue events
              // Format in packet: ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text
              // Need to convert to: Dialogue: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
              if (value && value.data && dataSize > 0) {
                const packetText = decoder.decode(value.data);
                
                if (packetCount <= 3) {
                  console.log(`Packet ${packetCount} decoded text:`, packetText);
                }
                
                // Format the dialogue line with proper ASS timing
                // Convert timestamp from seconds to ASS time format (H:MM:SS.CC)
                const formatASSTime = (seconds) => {
                  const hours = Math.floor(seconds / 3600);
                  const minutes = Math.floor((seconds % 3600) / 60);
                  const secs = Math.floor(seconds % 60);
                  const centisecs = Math.floor((seconds % 1) * 100);
                  return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}.${centisecs.toString().padStart(2, '0')}`;
                };
                
                const startTime = formatASSTime(value.timestamp || 0);
                const endTime = formatASSTime((value.timestamp || 0) + (value.duration || 0));
                
                // ASS packet format in MKV: ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text
                // Convert to: Dialogue: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
                // IMPORTANT: Text field can contain commas, so only split first 8 fields
                const trimmedText = packetText.trim();
                if (trimmedText) {
                  // Split only the first 8 fields, keeping everything after as Text
                  const commaIndex = [];
                  for (let i = 0; i < trimmedText.length; i++) {
                    if (trimmedText[i] === ',') commaIndex.push(i);
                    if (commaIndex.length === 8) break;
                  }
                  
                  if (commaIndex.length >= 8) {
                    // Extract fields: ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV, Effect
                    const readOrder = trimmedText.substring(0, commaIndex[0]);
                    const layer = trimmedText.substring(commaIndex[0] + 1, commaIndex[1]);
                    const style = trimmedText.substring(commaIndex[1] + 1, commaIndex[2]);
                    const name = trimmedText.substring(commaIndex[2] + 1, commaIndex[3]);
                    const marginL = trimmedText.substring(commaIndex[3] + 1, commaIndex[4]);
                    const marginR = trimmedText.substring(commaIndex[4] + 1, commaIndex[5]);
                    const marginV = trimmedText.substring(commaIndex[5] + 1, commaIndex[6]);
                    const effect = trimmedText.substring(commaIndex[6] + 1, commaIndex[7]);
                    const text = trimmedText.substring(commaIndex[7] + 1); // Everything after 8th comma
                    
                    // Format: Dialogue: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
                    dialogues += `Dialogue: ${layer},${startTime},${endTime},${style},${name},${marginL},${marginR},${marginV},${effect},${text}\n`;
                  } else {
                    // Fallback: just add as-is with Dialogue prefix and timing
                    dialogues += `Dialogue: 0,${startTime},${endTime},Default,,0,0,0,,${trimmedText}\n`;
                  }
                }
              }
            }
            
            if (packetCount >= maxPackets) {
              console.warn('Hit max packet limit, stopping read');
            }
          } finally {
            try {
              reader.releaseLock();
            } catch (e) {
              // Ignore lock release errors
            }
          }

          console.log(`Extracted ${packetCount} subtitle packets, ${totalBytes} total bytes, ${dialogues.length} bytes of dialogue data`);
          if (dialogues.length > 0) {
            console.log('Dialogue preview (first 500):', dialogues.substring(0, 500));
            console.log('Dialogue preview (last 500):', dialogues.substring(Math.max(0, dialogues.length - 500)));
          }
          
          // Combine header and dialogues
          return assHeader + dialogues;
        } catch (streamError) {
          console.error('Error reading subtitle stream:', streamError);
          console.error('Stream error details:', streamError.message, streamError.stack);
          // Return just the header if stream reading fails
          return assHeader;
        }
      } else if (this.demuxer) {
        // For other subtitle formats (SRT, SubRip, etc.)
        try {
          console.log('Reading non-ASS subtitle packets with readAVPacket...');
          const codec = track.codec?.toLowerCase() || '';
          const streamIndex = track.rawStream?.index ?? track.id;
          
          const stream = this.demuxer.readAVPacket(
            0, // start time
            this.mediaInfo.duration || 0, // end time
            3, // AVMediaType.AVMEDIA_TYPE_SUBTITLE
            streamIndex, // stream index
            1 // seek flag
          );
          const reader = stream.getReader();
          const decoder = new TextDecoder('utf-8');
          const subtitleEntries = [];
          let packetCount = 0;
          const maxPackets = 100000;

          try {
            while (packetCount < maxPackets) {
              const { done, value } = await reader.read();
              if (done) break;
              packetCount++;

              if (value && value.data && value.data.byteLength > 0) {
                const text = decoder.decode(value.data).trim();
                if (text) {
                  subtitleEntries.push({
                    start: value.timestamp || 0,
                    end: (value.timestamp || 0) + (value.duration || 0),
                    text: text
                  });
                }
              }
            }
          } finally {
            try {
              reader.releaseLock();
            } catch (e) {
              // Ignore lock release errors
            }
          }

          console.log(`Extracted ${subtitleEntries.length} subtitle entries for codec: ${codec}`);

          // Build SRT format output
          if (codec === 'srt' || codec === 'subrip' || codec === 'sub') {
            // Format as SRT
            const formatSRTTime = (seconds) => {
              const hours = Math.floor(seconds / 3600);
              const minutes = Math.floor((seconds % 3600) / 60);
              const secs = Math.floor(seconds % 60);
              const millis = Math.floor((seconds % 1) * 1000);
              return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')},${millis.toString().padStart(3, '0')}`;
            };

            let srtOutput = '';
            subtitleEntries.forEach((entry, index) => {
              srtOutput += `${index + 1}\n`;
              srtOutput += `${formatSRTTime(entry.start)} --> ${formatSRTTime(entry.end)}\n`;
              srtOutput += `${entry.text}\n\n`;
            });
            return srtOutput;
          }

          // For other formats, just return the raw text concatenated
          return subtitleEntries.map(e => e.text).join('\n');
        } catch (streamError) {
          console.error('Error reading subtitle stream:', streamError);
          return null;
        }
      }

      return null;
    } catch (error) {
      console.error('Error extracting subtitle track:', error);
      // Log full error details
      if (error.stack) {
        console.error('Error stack:', error.stack);
      }
      return null;
    }
  }

  async extractAttachment(index) {
    if (!this.attachments || index >= this.attachments.length) {
      return null;
    }

    try {
      const attachment = this.attachments[index];
      
      // The extradata contains the actual font file data
      if (attachment.data) {
        return {
          filename: attachment.filename,
          mimeType: attachment.mimeType,
          data: attachment.data
        };
      }
      
      return null;
    } catch (error) {
      console.error('Error extracting attachment:', error);
      return null;
    }
  }
  
  getAllAttachments() {
    return this.attachments || [];
  }

  async extractAndSaveFonts() {
    const savedFonts = [];
    
    console.log(`[Font Extractor] Starting extraction for ${this.attachments.length} attachments`);
    
    if (this.attachments.length === 0) {
      console.log('[Font Extractor] No attachments to process');
      return savedFonts;
    }
    
    try {
      // Test if Tauri is available
      console.log('[Font Extractor] Testing Tauri API availability...');
      await invoke('check_font_installed', { filename: 'test.ttf' }).catch(() => {});
      console.log('[Font Extractor] Tauri API is available');
    } catch (error) {
      console.error('[Font Extractor] Tauri API not available:', error);
      return savedFonts;
    }
    
    for (const attachment of this.attachments) {
      try {
        console.log(`[Font Extractor] Processing: ${attachment.filename}`);
        
        // Check if already installed on system
        const isInstalled = await invoke('check_font_installed', { 
          filename: attachment.filename 
        });
        
        if (isInstalled) {
          console.log(`[Font Extractor] ${attachment.filename} already on system, skipping`);
          savedFonts.push({
            filename: attachment.filename,
            location: 'system',
            skipped: true
          });
          continue;
        }
        
        // Extract font data
        const fontData = attachment.data;
        if (!fontData) {
          console.warn(`[Font Extractor] No data for ${attachment.filename}`);
          continue;
        }
        
        console.log(`[Font Extractor] Converting ${attachment.filename} (${fontData.byteLength || fontData.length} bytes)`);
        
        // Convert to regular array for Tauri
        const dataArray = Array.from(new Uint8Array(fontData));
        
        console.log(`[Font Extractor] Saving ${attachment.filename} to disk...`);
        
        // Save to app data fonts directory
        const savedPath = await invoke('save_font', {
          filename: attachment.filename,
          data: dataArray
        });
        
        console.log(`[Font Extractor] ✓ Saved ${attachment.filename} to ${savedPath}`);
        
        // Get the HTTP server port from torrent manager
        // Font will be served at http://localhost:{port}/fonts/{filename}
        const httpPort = await invoke('get_http_port');
        const httpUrl = `http://localhost:${httpPort}/fonts/${encodeURIComponent(attachment.filename)}`;
        
        console.log(`[Font Extractor] Font URL: ${httpUrl}`);
        
        savedFonts.push({
          filename: attachment.filename,
          location: savedPath,
          url: httpUrl,
          size: dataArray.length
        });
      } catch (error) {
        console.error(`[Font Extractor] Failed to save ${attachment.filename}:`, error);
      }
    }
    
    console.log(`[Font Extractor] Extraction complete. Saved ${savedFonts.length} fonts`);
    return savedFonts;
  }

  async embedFontsIntoASS(assData, fonts) {
    if (!fonts || fonts.length === 0) {
      console.log('[Font Embedder] No fonts to embed');
      return assData;
    }

    console.log(`[Font Embedder] Embedding ${fonts.length} fonts into ASS data`);
    
    // Read font files and encode as base64
    const fontSections = [];
    
    for (const font of fonts) {
      if (font.skipped || !font.location) continue;
      
      try {
        const fontBytes = await readBinaryFile(font.location);
        const base64 = btoa(String.fromCharCode(...new Uint8Array(fontBytes)));
        
        // Create font attachment section
        const fontSection = `[${font.filename}]\nfontname: ${font.filename}\n` +
          base64.match(/.{1,80}/g).join('\n');
        
        fontSections.push(fontSection);
        console.log(`[Font Embedder] Embedded ${font.filename}`);
      } catch (error) {
        console.error(`[Font Embedder] Failed to embed ${font.filename}:`, error);
      }
    }
    
    if (fontSections.length === 0) {
      return assData;
    }
    
    // Insert font sections before [Events] section
    const eventsIndex = assData.indexOf('[Events]');
    if (eventsIndex === -1) {
      console.warn('[Font Embedder] No [Events] section found in ASS data');
      return assData;
    }
    
    const beforeEvents = assData.substring(0, eventsIndex);
    const afterEvents = assData.substring(eventsIndex);
    const fontsSection = '\n' + fontSections.join('\n\n') + '\n\n';
    
    console.log(`[Font Embedder] Successfully embedded ${fontSections.length} fonts`);
    return beforeEvents + fontsSection + afterEvents;
  }

  createADTSHeader(frameLength, sampleRate, channels) {
    // ADTS header is 7 bytes
    const header = new ArrayBuffer(7);
    const view = new DataView(header);
    
    // Sample rate index lookup
    const sampleRateIndex = {
      96000: 0, 88200: 1, 64000: 2, 48000: 3,
      44100: 4, 32000: 5, 24000: 6, 22050: 7,
      16000: 8, 12000: 9, 11025: 10, 8000: 11, 7350: 12
    }[sampleRate] || 4; // Default to 44100
    
    const fullLength = frameLength + 7;
    
    // Syncword (12 bits): 0xFFF
    view.setUint8(0, 0xFF);
    view.setUint8(1, 0xF0 | 0x01); // MPEG-4, no CRC
    
    // Profile (2 bits): AAC LC = 1, Layer (2 bits): 0, Protection absent (1 bit): 1
    // Sample rate index (4 bits), Private (1 bit): 0
    view.setUint8(2, (1 << 6) | (sampleRateIndex << 2) | (channels >> 2));
    
    // Channel config (3 bits), Originality (1 bit), Home (1 bit), Copyright ID (1 bit), Copyright start (1 bit), Frame length (2 bits high)
    view.setUint8(3, ((channels & 0x3) << 6) | (fullLength >> 11));
    
    // Frame length (8 bits middle)
    view.setUint8(4, (fullLength >> 3) & 0xFF);
    
    // Frame length (3 bits low), Buffer fullness (5 bits high)
    view.setUint8(5, ((fullLength & 0x7) << 5) | 0x1F);
    
    // Buffer fullness (6 bits low), Number of frames (2 bits): 0
    view.setUint8(6, 0xFC);
    
    return header;
  }

  async extractAudioTrack(trackIndex) {
    if (!this.audioTracks[trackIndex]) return null;

    try {
      const track = this.audioTracks[trackIndex];
      console.log(`Extracting audio track ${trackIndex}, stream index ${track.id}, codec: ${track.codec}`);
      
      if (!this.demuxer) {
        console.error('No demuxer available');
        return null;
      }

      const streamIndex = track.rawStream?.index ?? track.id;
      
      // readAVPacket signature: (start, end, streamType, streamIndex, seekFlag)
      // AVMediaType: AVMEDIA_TYPE_AUDIO = 1
      const stream = this.demuxer.readAVPacket(
        0, // start time
        this.mediaInfo.duration || 0, // end time
        1, // AVMediaType.AVMEDIA_TYPE_AUDIO
        streamIndex,
        1 // seek flag
      );
      
      const reader = stream.getReader();
      const chunks = [];
      let packetCount = 0;
      let totalBytes = 0;

      try {
        const maxPackets = 1000000; // Large limit for audio
        
        while (packetCount < maxPackets) {
          const { done, value } = await reader.read();
          if (done) {
            console.log('Audio stream reading complete');
            break;
          }

          packetCount++;
          
          if (value && value.data && value.data.byteLength > 0) {
            totalBytes += value.data.byteLength;
            
            // Store packet data with metadata
            chunks.push({
              data: new Uint8Array(value.data),
              timestamp: value.timestamp || 0,
              duration: value.duration || 0,
              keyframe: value.keyframe || false
            });
          }
          
          if (packetCount % 1000 === 0) {
            console.log(`Extracted ${packetCount} audio packets, ${totalBytes} bytes`);
          }
        }
        
        console.log(`Extracted ${packetCount} audio packets, ${totalBytes} total bytes`);
        
        // For AAC, we need to add ADTS headers to each packet
        if (track.codec.toLowerCase() === 'aac') {
          console.log('Adding ADTS headers for AAC audio');
          const adtsPackets = [];
          for (const packet of chunks) {
            const adtsHeader = this.createADTSHeader(packet.data.byteLength, track.sampleRate, track.channels);
            // Concatenate ADTS header + packet data
            const adtsPacket = new Uint8Array(adtsHeader.byteLength + packet.data.byteLength);
            adtsPacket.set(new Uint8Array(adtsHeader), 0);
            adtsPacket.set(packet.data, adtsHeader.byteLength);
            adtsPackets.push({
              ...packet,
              data: adtsPacket
            });
          }
          chunks.length = 0;
          chunks.push(...adtsPackets);
        }
        
        // Return audio data with track metadata
        return {
          codec: track.codec,
          sampleRate: track.sampleRate,
          channels: track.channels,
          extradata: track.extradata,
          packets: chunks,
          totalBytes: totalBytes,
          duration: this.mediaInfo.duration
        };
      } finally {
        try {
          reader.releaseLock();
        } catch (e) {
          // Ignore
        }
      }
    } catch (error) {
      console.error('Error extracting audio track:', error);
      if (error.stack) {
        console.error('Error stack:', error.stack);
      }
      return null;
    }
  }

  async startStreamingAudio(trackIndex, startTime = 0) {
    if (!this.audioTracks[trackIndex]) {
      console.error(`Audio track ${trackIndex} not found`);
      return false;
    }

    try {
      const track = this.audioTracks[trackIndex];
      console.log(`Starting audio stream for track ${trackIndex}, stream index ${track.id}, codec: ${track.codec}`);
      
      if (!this.demuxer) {
        console.error('No demuxer available');
        return false;
      }

      // Stop all current streams (video + audio)
      this.stop();

      // Restart video from current position
      if (this.videoTrack) {
        this.videoStream = this.demuxer.read('video', startTime, this.videoTrack.id);
        this.readVideoStream();
      }

      // Start streaming the new audio track
      const stream = this.demuxer.read('audio', startTime, track.id);
      if (!stream) {
        console.error('Failed to create audio stream');
        return false;
      }

      this.audioStreams.set(track.id, stream);
      this.readAudioStream(track.id, stream);
      
      console.log(`✓ Started streaming audio track ${trackIndex}`);
      return true;
    } catch (error) {
      console.error('Error starting audio stream:', error);
      return false;
    }
  }

  stopAllAudio() {
    // Cancel all audio streams
    for (const [trackId, signal] of this.cancelSignals.entries()) {
      this.cancelSignals.set(trackId, true);
    }
    
    // Release all audio readers
    for (const [trackId, reader] of this.audioReaders.entries()) {
      try {
        reader.releaseLock();
      } catch (e) {
        // Already released
      }
    }
    
    this.audioReaders.clear();
    this.audioStreams.clear();
  }

  destroy() {
    this.stop();

    if (this.demuxer) {
      this.demuxer.destroy();
      this.demuxer = null;
    }
  }
}
