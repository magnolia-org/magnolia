import SubtitlesOctopus from '@jellyfin/libass-wasm';

export class SubtitleRenderer {
  constructor(canvas, videoElement) {
    this.videoElement = videoElement;
    this.octopus = null;
    this.customFonts = [];
    this.offset = 0;
    this.lastSubtitleData = null;
    this.lastCodec = null;
  }

  setFonts(fontPaths) {
    this.customFonts = fontPaths || [];
    console.log('[SubtitleRenderer] Set custom fonts:', this.customFonts);
  }

  setOffset(offset) {
    if (this.offset === offset) return;
    this.offset = offset;
    console.log('[SubtitleRenderer] Setting offset:', offset);
    if (this.lastSubtitleData) {
      this.loadSubtitleTrack(this.lastSubtitleData, this.lastCodec);
    }
  }

  async initialize() {
    if (!this.videoElement) {
      throw new Error('Video element required');
    }

    if (!this.videoElement.parentElement) {
      throw new Error('Video element must be in DOM');
    }

    // Dispose of any existing instance first
    this.dispose();

    const fontList = this.customFonts.length > 0 
      ? this.customFonts 
      : ['/fonts/geist-sans.woff2', '/fonts/.fallback-default.woff2'];
    
    console.log('[SubtitleRenderer] Initializing with fonts:', fontList);
    
    this.octopus = new SubtitlesOctopus({
      video: this.videoElement,
      subContent: '[Script Info]\nTitle: Default\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n',
      fonts: fontList,
      workerUrl: '/subtitles-octopus-worker.js',
      legacyWorkerUrl: '/subtitles-octopus-worker-legacy.js',
    });

    console.log('SubtitleRenderer initialized');
    return this;
  }

  async loadSubtitleTrack(subtitleData, codec = 'ass') {
    // Store for re-initialization on offset change
    this.lastSubtitleData = subtitleData;
    this.lastCodec = codec;

    // Reinitialize SubtitlesOctopus for each new track to avoid corruption
    // This is needed because setTrack() doesn't properly reset internal state
    console.log(`Loading ${codec} subtitles (${subtitleData.length} bytes) - reinitializing renderer with offset ${this.offset}`);
    
    // Store video element reference before dispose
    const videoEl = this.videoElement;
    
    // Dispose the old instance
    this.dispose();
    
    // Restore video element reference
    this.videoElement = videoEl;
    
    // Create new instance with the subtitle data
    if (!this.videoElement) {
      throw new Error('Video element required');
    }

    if (!this.videoElement.parentElement) {
      throw new Error('Video element must be in DOM');
    }

    const fontList = this.customFonts.length > 0 
      ? this.customFonts 
      : ['/fonts/geist-sans.woff2', '/fonts/.fallback-default.woff2'];
    
    console.log('[SubtitleRenderer] Loading track with fonts:', fontList);
    
    this.octopus = new SubtitlesOctopus({
      video: this.videoElement,
      subContent: subtitleData,
      fonts: fontList,
      workerUrl: '/subtitles-octopus-worker.js',
      legacyWorkerUrl: '/subtitles-octopus-worker-legacy.js',
      timeOffset: this.offset,
    });

    console.log('SubtitleRenderer reinitialized with new track');
  }

  show() {
    this.setVisible(true);
  }

  hide() {
    this.setVisible(false);
  }

  setVisible(visible) {
    if (this.octopus?.canvas) {
      this.octopus.canvas.style.display = visible ? 'block' : 'none';
    }
  }

  dispose() {
    if (this.octopus) {
      try {
        this.octopus.dispose();
      } catch (e) {
        console.warn('Error disposing SubtitlesOctopus:', e);
      }
      this.octopus = null;
    }
  }

  isReady() {
    return this.octopus !== null;
  }

  getCanvas() {
    return this.octopus?.canvas || null;
  }

  updateTime(time) {
    // SubtitlesOctopus handles time updates automatically via the video element
    // This method exists for compatibility but does nothing
  }
}
