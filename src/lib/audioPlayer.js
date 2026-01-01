export class AudioPlayer {
  constructor() {
    this.audioContext = null;
    this.sourceNode = null;
    this.gainNode = null;
    this.analyserNode = null;
    this.currentTrackId = null;
    this.audioQueue = [];
    this.isPlaying = false;
    this.volume = 1.0;
    this.muted = false;
    this.sampleRate = 48000;
    this.channels = 2;
    this.scheduledTime = 0;
    this.startTime = 0;
    this.audioDecoder = null;
    this.pendingFrames = [];
    this.codecConfig = null;
  }

  async initialize() {
    this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
    
    this.gainNode = this.audioContext.createGain();
    this.analyserNode = this.audioContext.createAnalyser();
    
    this.gainNode.connect(this.analyserNode);
    this.analyserNode.connect(this.audioContext.destination);
    
    this.gainNode.gain.value = this.volume;
    
    if (typeof AudioDecoder === 'undefined') {
      console.error('WebCodecs API not available in this browser');
      throw new Error('WebCodecs API not supported');
    }
    
    return this;
  }

  setTrack(trackId, trackInfo) {
    this.currentTrackId = trackId;
    this.sampleRate = trackInfo.sampleRate || 48000;
    this.channels = trackInfo.channels || 2;
    
    console.log('Setting audio track:', trackId, 'Sample rate:', this.sampleRate, 'Channels:', this.channels);
    console.log('Track codec:', trackInfo.codec);
    if (trackInfo.extradata) {
      console.log('Track has extradata:', trackInfo.extradata.byteLength || trackInfo.extradata.length, 'bytes');
    } else {
      console.log('Track has NO extradata');
    }
    
    this.audioQueue = [];
    this.pendingFrames = [];
    this.scheduledTime = this.audioContext.currentTime;
    
    // Initialize AudioDecoder with codec info
    this.initializeDecoder(trackInfo);
  }

  initializeDecoder(trackInfo) {
    if (this.audioDecoder) {
      try {
        this.audioDecoder.close();
      } catch (e) {
        console.warn('Error closing previous decoder:', e);
      }
      this.audioDecoder = null;
    }

    const codecMap = {
      'aac': 'mp4a.40.2',
      'opus': 'opus',
      'vorbis': 'vorbis',
      'mp3': 'mp3',
      'flac': 'flac'
    };

    const codec = codecMap[trackInfo.codec?.toLowerCase()] || trackInfo.codec;
    
    this.codecConfig = {
      codec: codec,
      sampleRate: this.sampleRate,
      numberOfChannels: this.channels
    };

    if (trackInfo.extradata && (trackInfo.codec?.toLowerCase() === 'aac')) {
      this.codecConfig.description = trackInfo.extradata;
      console.log('Adding AAC description data, size:', trackInfo.extradata.byteLength || trackInfo.extradata.length);
    }

    console.log('Initializing AudioDecoder with config:', this.codecConfig);

    try {
      this.audioDecoder = new AudioDecoder({
        output: (audioData) => {
          this.handleDecodedAudio(audioData);
        },
        error: (error) => {
          console.error('AudioDecoder error:', error);
        }
      });

      this.audioDecoder.configure(this.codecConfig);
      console.log('AudioDecoder initialized successfully');
    } catch (error) {
      console.error('Failed to initialize AudioDecoder:', error);
      this.audioDecoder = null;
    }
  }

  handleDecodedAudio(audioData) {
    try {
      const audioBuffer = this.audioContext.createBuffer(
        audioData.numberOfChannels,
        audioData.numberOfFrames,
        audioData.sampleRate
      );

      for (let channel = 0; channel < audioData.numberOfChannels; channel++) {
        const channelData = new Float32Array(audioData.numberOfFrames);
        audioData.copyTo(channelData, { planeIndex: channel, format: 'f32-planar' });
        audioBuffer.copyToChannel(channelData, channel);
      }

      audioData.close();

      // Schedule the audio buffer for playback
      if (this.isPlaying) {
        this.scheduleAudioBuffer(audioBuffer);
      }
    } catch (error) {
      console.error('Error handling decoded audio:', error);
    }
  }

  async decodeAndScheduleAudio(samples) {
    if (!samples || samples.length === 0) {
      console.warn('decodeAndScheduleAudio called with no samples');
      return;
    }
    
    if (!this.audioDecoder) {
      console.error('AudioDecoder not initialized!');
      return;
    }
    
    if (this.audioDecoder.state !== 'configured') {
      console.error('AudioDecoder not in configured state:', this.audioDecoder.state);
      return;
    }

    console.log(`Received ${samples.length} audio samples for decoding`);

    for (const sample of samples) {
      try {
        if (!sample.data || sample.data.byteLength === 0) {
          console.warn('Empty audio sample data');
          continue;
        }

        console.log('Processing audio sample:', {
          hasData: !!sample.data,
          dataSize: sample.data.byteLength,
          timestamp: sample.timestamp,
          duration: sample.duration,
          isSync: sample.is_sync,
          type: sample.type
        });

        const chunk = new EncodedAudioChunk({
          type: sample.is_sync || sample.type === 'key' ? 'key' : 'delta',
          timestamp: (sample.timestamp || 0) * 1000000,
          duration: (sample.duration || 0) * 1000000,
          data: sample.data
        });

        this.audioDecoder.decode(chunk);
      } catch (error) {
        console.error('Error decoding audio sample:', error, sample);
      }
    }

    if (this.audioDecoder && this.audioDecoder.state === 'configured') {
      try {
        await this.audioDecoder.flush();
      } catch (e) {
        console.warn('Error flushing decoder:', e);
      }
    }
  }

  scheduleAudioBuffer(audioBuffer) {
    if (!this.isPlaying) return;

    const source = this.audioContext.createBufferSource();
    source.buffer = audioBuffer;
    source.connect(this.gainNode);

    if (this.scheduledTime < this.audioContext.currentTime) {
      this.scheduledTime = this.audioContext.currentTime;
    }

    source.start(this.scheduledTime);
    this.scheduledTime += audioBuffer.duration;

    source.onended = () => {
      source.disconnect();
    };
  }

  play() {
    if (this.audioContext.state === 'suspended') {
      this.audioContext.resume();
    }
    this.isPlaying = true;
    this.startTime = this.audioContext.currentTime;
  }

  pause() {
    this.isPlaying = false;
    if (this.audioContext.state === 'running') {
      this.audioContext.suspend();
    }
  }

  stop() {
    this.isPlaying = false;
    this.audioQueue = [];
    this.pendingFrames = [];
    
    if (this.sourceNode) {
      try {
        this.sourceNode.stop();
        this.sourceNode.disconnect();
      } catch (e) {
        console.warn('Error stopping audio source:', e);
      }
      this.sourceNode = null;
    }
    
    this.scheduledTime = this.audioContext.currentTime;
  }

  clearBuffers() {
    this.audioQueue = [];
    this.pendingFrames = [];
    this.scheduledTime = this.audioContext.currentTime;
    
    if (this.audioDecoder && this.audioDecoder.state === 'configured') {
      try {
        this.audioDecoder.reset();
      } catch (e) {
        console.warn('Error resetting decoder:', e);
      }
    }
  }

  setVolume(volume) {
    this.volume = Math.max(0, Math.min(1, volume));
    if (this.gainNode) {
      this.gainNode.gain.setValueAtTime(this.muted ? 0 : this.volume, this.audioContext.currentTime);
    }
  }

  setMuted(muted) {
    this.muted = muted;
    if (this.gainNode) {
      this.gainNode.gain.setValueAtTime(muted ? 0 : this.volume, this.audioContext.currentTime);
    }
  }

  getCurrentTime() {
    if (!this.isPlaying) return 0;
    return this.audioContext.currentTime - this.startTime;
  }

  seek(time) {
    this.stop();
    this.scheduledTime = this.audioContext.currentTime;
    this.startTime = this.audioContext.currentTime - time;
    
    if (this.audioDecoder && this.audioDecoder.state === 'configured') {
      try {
        this.audioDecoder.reset();
        this.audioDecoder.configure(this.codecConfig);
      } catch (e) {
        console.warn('Error resetting decoder on seek:', e);
      }
    }
    
    if (this.isPlaying) {
      this.play();
    }
  }

  getAnalyserData() {
    if (!this.analyserNode) return null;
    
    const dataArray = new Uint8Array(this.analyserNode.frequencyBinCount);
    this.analyserNode.getByteFrequencyData(dataArray);
    return dataArray;
  }

  dispose() {
    this.stop();
    
    if (this.audioDecoder) {
      try {
        this.audioDecoder.close();
      } catch (e) {
        console.warn('Error closing audio decoder:', e);
      }
      this.audioDecoder = null;
    }
    
    if (this.gainNode) {
      this.gainNode.disconnect();
      this.gainNode = null;
    }
    
    if (this.analyserNode) {
      this.analyserNode.disconnect();
      this.analyserNode = null;
    }
    
    if (this.audioContext) {
      this.audioContext.close();
      this.audioContext = null;
    }
  }
}
