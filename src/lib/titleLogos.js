import { tmdbGet, getImageUrl, getCorsImageUrl } from './tmdb.js';
import { loadImage } from './utils/colorUtils.js';

// one lookup per title for the whole session, shared by every tile showing it
const cache = new Map();

function pickLogo(logos) {
  const usable = (logos || []).filter((logo) => logo.iso_639_1 === 'en' || logo.iso_639_1 === null);
  usable.sort((a, b) =>
    (b.iso_639_1 === 'en') - (a.iso_639_1 === 'en') || (b.vote_average || 0) - (a.vote_average || 0));
  return usable[0] || null;
}

// many logos are black lettering for light backgrounds; those get drawn white over the art
async function isDarkLogo(path) {
  try {
    const img = await loadImage(getCorsImageUrl(path, 'w185'));
    const canvas = document.createElement('canvas');
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    ctx.drawImage(img, 0, 0);
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let total = 0;
    let weight = 0;
    for (let i = 0; i < data.length; i += 16) {
      const alpha = data[i + 3] / 255;
      if (alpha < 0.5) continue;
      total += (0.2126 * data[i] + 0.7152 * data[i + 1] + 0.0722 * data[i + 2]) * alpha;
      weight += alpha;
    }
    return weight > 0 && total / weight < 60;
  } catch {
    return false;
  }
}

// resolves to { url, invert } or null when tmdb has no english logo for the title
export function getTitleLogo(item) {
  const mediaType = item.media_type === 'movie' ? 'movie' : 'tv';
  const key = `${item.id}-${mediaType}`;
  if (!cache.has(key)) {
    cache.set(key, (async () => {
      try {
        const images = await tmdbGet(`${mediaType}/${item.id}/images`, { include_image_language: 'en,null' });
        const logo = pickLogo(images?.logos);
        if (!logo) return null;
        return { url: getImageUrl(logo.file_path, 'w500'), invert: await isDarkLogo(logo.file_path) };
      } catch {
        // let a later tile try again
        cache.delete(key);
        return null;
      }
    })());
  }
  return cache.get(key);
}
