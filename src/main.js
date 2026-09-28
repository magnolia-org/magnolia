import './styles/main.css'
import './styles/video-player.css'
import './styles/search.css'
import './styles/home.css'
import './styles/media-detail.css'
import './styles/view-all.css'
import './styles/torrent-debug.css'
import './styles/file-selector.css'
import './styles/fonts.css'
import 'remixicon/fonts/remixicon.css'
import { mount } from 'svelte'
import App from './App.svelte'
import { isMacOS } from './lib/utils/platform.js'

document.addEventListener('contextmenu', (e) => e.preventDefault());

// Tag the document root with the current platform so CSS can apply
// platform-specific rules (e.g. traffic-light padding on macOS,
// backdrop-filter workarounds on Windows/Linux).
if (isMacOS) {
  document.documentElement.classList.add('macos');
}

const app = mount(App, {
  target: document.getElementById('app'),
})

export default app

;(() => { let n = 0; const t0 = performance.now(); const tick = (t) => { n++; if (t - t0 < 2000) requestAnimationFrame(tick); else console.log('[fps-probe]', Math.round(n / ((t - t0) / 1000))); }; setTimeout(() => requestAnimationFrame(tick), 1500); })();
