
<div align="center">
  <img src="src/media/magnolia.png" alt="Magnolia Logo" width="150"/>
  <p>
  <h1>Magnolia</h1>
  <p><strong>A beautiful, feature-rich torrent streaming program</strong></p>
  <p>(image(s) here?)</p>
</div>

## Features

- Direct streaming of torrents without full download
- Search for media from multiple torrent providers directly in-app
    - Nyaa
    - LimeTorrents
    - ThePirateBay
    - EZTV
- Advanced video player
    - Full SRT and ASS subtitle support
    - Audio track support
    - Chapter support with automatic skip prompts
    - Keyboard shortcuts
- Modern user interface
- Watch progress tracking
- Recommendations based on your "my list"

## Download
### Get the latest build [here](https://github.com/chwair/magnolia/releases/latest)
(Windows only for now, MacOS and Linux support is planned)

## Building

### Prerequisites

- Node.js 18+
- Rust 1.75+
- pnpm/npm

### Setup

```bash
# Clone the repository
git clone https://github.com/chwair/magnolia.git
cd magnolia

# Install dependencies
npm install

# Run dev server
npm run tauri:dev

# Build for production
npm run tauri:build
```
## Acknowledgments

- [TMDB](https://www.themoviedb.org/)
- [web-demuxer](https://github.com/bilibili/web-demuxer)
- [SubtitlesOctopus](https://github.com/jellyfin/JavascriptSubtitlesOctopus)
- [FFmpeg](https://ffmpeg.org/)
- [Tauri](https://tauri.app/)
- [Wyzie Subs](https://github.com/itzCozi/wyzie-subs)

## License

MIT

## Disclaimer

Magnolia doesn't host any files or torrents. It is the user's responsibility to ensure they have the legal right to download and stream any content accessed through the application. Please adhere to your local copyright laws and regulations.