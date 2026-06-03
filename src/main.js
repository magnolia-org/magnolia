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

// document.addEventListener('contextmenu', (e) => e.preventDefault());

const app = mount(App, {
  target: document.getElementById('app'),
})

export default app
