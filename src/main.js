import './styles/main.css'
import './styles/video-player.css'
import './styles/search.css'
import './styles/home.css'
import './styles/media-detail.css'
import './styles/view-all.css'
import './styles/torrent-debug.css'
import './styles/file-selector.css'
import '@fontsource/geist-sans/400.css'
import '@fontsource/geist-sans/500.css'
import '@fontsource/geist-sans/600.css'
import '@fontsource/geist-sans/700.css'
import '@fontsource-variable/geist-mono'
import 'remixicon/fonts/remixicon.css'
import App from './App.svelte'

document.addEventListener('contextmenu', (e) => e.preventDefault());

const app = new App({
  target: document.getElementById('app'),
})

export default app
