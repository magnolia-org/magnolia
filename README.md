
<div align="center">
  <img src="src/media/magnolia.png" alt="Magnolia Logo" height="110"/>
  <p>
  <h1>Magnolia</h1>
  <p><strong>A beautiful, feature-rich torrent streaming client</strong></p>
  <p><img height=800 alt="Magnolia's home page" src="https://github.com/user-attachments/assets/430bcd15-f9a0-4e8b-9827-cf91514afaca"/></p>
</div>

## Features

- Direct streaming of torrents
- Search for media from multiple torrent providers directly in-app
    - Nyaa
    - LimeTorrents
    - ThePirateBay
    - EZTV
- Support for debrid services, with TorBox built in
- Extension support for adding custom torrent providers, subtitle sources and debrid services
- Video playback through mpv
- Manage multiple torrents per season/episode for episodic media
- Import subtitles individually or from a folder for a full series
- Modern user interface
- Watch progress tracking
- Recommendations based on your "my list"

## Download
### Get the latest build [here](https://github.com/chwair/magnolia/releases/latest)
(Windows, MacOS (Apple Silicon) and Linux (.deb) support.)<br>
You can also get it through the AUR if using Arch.
```bash
yay -S magnolia-bin
```

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

# Install utils for soia
npm run setup:libs

# Run dev server
npm run tauri:dev

# Build for production
npm run tauri:build
```

## Acknowledgments

- [TMDB](https://www.themoviedb.org/)
- [Soia](https://github.com/FengZeng/soia)
- [rqbit](https://github.com/ikatson/rqbit)
- [mpv](https://github.com/mpv-player/mpv)
- [Tauri](https://tauri.app/)

## License

Magnolia is dual-licensed under MIT and GPL-3.0. If using components containing Soia, please include the GPL-3.0 license in your fork.

## Disclaimer

Magnolia doesn't host any files or torrents. It is the user's responsibility to ensure they have the legal right to download and stream any content accessed through the application. Please adhere to your local copyright laws and regulations.
