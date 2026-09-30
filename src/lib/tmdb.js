const TMDB_BASE_URL = 'https://api.themoviedb.org/3';
const TMDB_IMAGE_BASE_URL = 'https://image.tmdb.org/t/p';
const TOKEN_ENDPOINT = 'https://magnoliatmdb.wyziemagnolia.workers.dev/tmdb-proxy';

// every request asks for english titles and overviews, falling back to the original when tmdb has none
const LANGUAGE = 'en-US';

let cachedToken = null;

async function getBearerToken() {
  if (cachedToken) {
    return cachedToken;
  }

  try {
    const response = await fetch(TOKEN_ENDPOINT);
    const data = await response.json();
    if (data.token) {
      cachedToken = data.token;
      return cachedToken;
    }
    throw new Error('no token in response');
  } catch (error) {
    console.error('failed to fetch bearer token:', error);
    throw error;
  }
}

async function getHeaders() {
  const token = await getBearerToken();
  return {
    'Authorization': `Bearer ${token}`,
    'Content-Type': 'application/json;charset=utf-8'
  };
}

export async function tmdbGet(path, params = {}) {
  const query = new URLSearchParams({ language: LANGUAGE });
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') query.set(key, String(value));
  }
  const url = `${TMDB_BASE_URL}/${path}?${query}`;

  let response = await fetch(url, { headers: await getHeaders() });
  if (response.status === 401) {
    // the proxy rotates tokens, so drop the cached one and retry once
    cachedToken = null;
    response = await fetch(url, { headers: await getHeaders() });
  }
  return response.json();
}

export function getImageUrl(path, size = 'w500') {
  if (!path) return null;
  return `${TMDB_IMAGE_BASE_URL}/${size}${path}`;
}

// for canvas color reads: a plain <img> of the same url poisons the cache and the cors load fails,
// so the query string keeps cors loads in their own cache entry
export function getCorsImageUrl(path, size = 'w500') {
  const url = getImageUrl(path, size);
  return url ? `${url}?cors=1` : null;
}

export function getConfiguration() {
  return tmdbGet('configuration');
}

export function getTrending(mediaType = 'all', timeWindow = 'day', page = 1) {
  return tmdbGet(`trending/${mediaType}/${timeWindow}`, { page });
}

export function getPopularMovies(page = 1) {
  return tmdbGet('movie/popular', { page });
}

export function getPopularTV(page = 1) {
  return tmdbGet('tv/popular', { page });
}

export function getTopRatedMovies(page = 1) {
  return tmdbGet('movie/top_rated', { page });
}

export function getTopRatedTV(page = 1) {
  return tmdbGet('tv/top_rated', { page });
}

export function getNowPlaying(page = 1) {
  return tmdbGet('movie/now_playing', { page, region: 'US' });
}

export function discoverMovies(params = {}) {
  return tmdbGet('discover/movie', params);
}

export function discoverTV(params = {}) {
  return tmdbGet('discover/tv', params);
}

export function getMovieDetails(movieId) {
  return tmdbGet(`movie/${movieId}`, {
    append_to_response: 'credits,videos,images,similar,keywords,recommendations',
    include_image_language: 'en,null',
  });
}

export function getTVDetails(tvId) {
  return tmdbGet(`tv/${tvId}`, {
    append_to_response: 'credits,videos,images,similar,aggregate_credits,keywords,recommendations',
    include_image_language: 'en,null',
  });
}

export function getSeasonDetails(tvId, seasonNumber) {
  return tmdbGet(`tv/${tvId}/season/${seasonNumber}`);
}

export function getEpisodeDetails(tvId, seasonNumber, episodeNumber) {
  return tmdbGet(`tv/${tvId}/season/${seasonNumber}/episode/${episodeNumber}`);
}

export function searchMulti(query, page = 1) {
  return tmdbGet('search/multi', { query, page });
}

export function searchMovies(query, page = 1) {
  return tmdbGet('search/movie', { query, page });
}

export function searchTV(query, page = 1) {
  return tmdbGet('search/tv', { query, page });
}

export function getMovieGenres() {
  return tmdbGet('genre/movie/list');
}

export function getTVGenres() {
  return tmdbGet('genre/tv/list');
}

export function getMovieCredits(movieId) {
  return tmdbGet(`movie/${movieId}/credits`);
}

export function getTVCredits(tvId) {
  return tmdbGet(`tv/${tvId}/credits`);
}

export function getMovieRecommendations(movieId, page = 1) {
  return tmdbGet(`movie/${movieId}/recommendations`, { page });
}

export function getTVRecommendations(tvId, page = 1) {
  return tmdbGet(`tv/${tvId}/recommendations`, { page });
}

export function getSimilarMovies(movieId, page = 1) {
  return tmdbGet(`movie/${movieId}/similar`, { page });
}

export function getSimilarTV(tvId, page = 1) {
  return tmdbGet(`tv/${tvId}/similar`, { page });
}

export function getMovieExternalIds(movieId) {
  return tmdbGet(`movie/${movieId}/external_ids`);
}

export function getTVExternalIds(tvId) {
  return tmdbGet(`tv/${tvId}/external_ids`);
}

export function getMovieKeywords(movieId) {
  return tmdbGet(`movie/${movieId}/keywords`);
}

export function getTVKeywords(tvId) {
  return tmdbGet(`tv/${tvId}/keywords`);
}

export function getEpisodeExternalIds(tvId, seasonNumber, episodeNumber) {
  return tmdbGet(`tv/${tvId}/season/${seasonNumber}/episode/${episodeNumber}/external_ids`);
}

export function getPersonCredits(personId) {
  return tmdbGet(`person/${personId}/combined_credits`);
}
