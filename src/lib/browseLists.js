import { tmdbGet, getPersonCredits } from './tmdb.js';

const DAY_MS = 24 * 60 * 60 * 1000;
const ANIMATION_GENRE = 16;

// kids, news, reality, soap and talk shows crowd tmdb's tv lists without being worth streaming
const TV_NOISE_GENRES = '10762,10763,10764,10766,10767';

function isoDate(offsetDays = 0) {
  return new Date(Date.now() + offsetDays * DAY_MS).toISOString().slice(0, 10);
}

function genreIds(item) {
  return item.genre_ids || item.genres?.map((genre) => genre.id) || [];
}

export function isAnime(item) {
  return item.original_language === 'ja' && genreIds(item).includes(ANIMATION_GENRE);
}

// the home lists are built around english titles plus anime, which is what the trackers carry best
export function isEnglishFriendly(item) {
  return item.original_language === 'en' || isAnime(item);
}

export function isReleased(item) {
  const date = item.release_date || item.first_air_date;
  return !!date && date <= isoDate();
}

function withType(results, mediaType) {
  return (results || []).map((item) => (item.media_type ? item : { ...item, media_type: mediaType }));
}

function page(response, results) {
  return { results, total_pages: Math.min(response?.total_pages || 1, 500) };
}

// params is a function so date windows are worked out when the list loads, not at import
function discover(mediaType, params) {
  return async (pageNumber) => {
    const response = await tmdbGet(`discover/${mediaType}`, { ...params(), page: pageNumber });
    return page(response, withType(response?.results, mediaType));
  };
}

function trending(mediaType, timeWindow, keep) {
  return async (pageNumber) => {
    const response = await tmdbGet(`trending/${mediaType}/${timeWindow}`, { page: pageNumber });
    const results = withType(response?.results, mediaType).filter(
      (item) =>
        (item.media_type === 'movie' || item.media_type === 'tv') &&
        isReleased(item) &&
        (item.vote_count || 0) >= 10 &&
        keep(item),
    );
    return page(response, results);
  };
}

const LISTS = {
  trending: trending('all', 'day', isEnglishFriendly),

  trending_shows: trending('tv', 'week', (item) => {
    const noise = TV_NOISE_GENRES.split(',').map(Number);
    return item.original_language === 'en' && !genreIds(item).some((id) => noise.includes(id));
  }),

  // recent digital and physical releases in the us, the ones that actually have good sources
  new_releases: discover('movie', () => ({
    region: 'US',
    with_release_type: '4|5',
    'release_date.gte': isoDate(-90),
    'release_date.lte': isoDate(),
    with_original_language: 'en',
    'vote_count.gte': 20,
    sort_by: 'popularity.desc',
  })),

  popular_movies: discover('movie', () => ({
    with_original_language: 'en',
    'primary_release_date.gte': isoDate(-365),
    'primary_release_date.lte': isoDate(),
    'vote_count.gte': 100,
    sort_by: 'popularity.desc',
  })),

  top_movies: discover('movie', () => ({
    with_original_language: 'en',
    without_genres: '99,10770',
    'vote_count.gte': 3000,
    sort_by: 'vote_average.desc',
  })),

  top_shows: discover('tv', () => ({
    with_original_language: 'en',
    without_genres: `${TV_NOISE_GENRES},99`,
    'vote_count.gte': 1000,
    sort_by: 'vote_average.desc',
  })),

  airing_anime: discover('tv', () => ({
    with_genres: ANIMATION_GENRE,
    with_original_language: 'ja',
    without_genres: '10762',
    'air_date.gte': isoDate(-7),
    'air_date.lte': isoDate(7),
    'vote_count.gte': 20,
    sort_by: 'popularity.desc',
  })),

  popular_anime: discover('tv', () => ({
    with_genres: ANIMATION_GENRE,
    with_original_language: 'ja',
    without_genres: '10762',
    'vote_count.gte': 200,
    sort_by: 'popularity.desc',
  })),

  top_anime: discover('tv', () => ({
    with_genres: ANIMATION_GENRE,
    with_original_language: 'ja',
    'vote_count.gte': 500,
    sort_by: 'vote_average.desc',
  })),

  anime_movies: discover('movie', () => ({
    with_genres: ANIMATION_GENRE,
    with_original_language: 'ja',
    'vote_count.gte': 100,
    sort_by: 'popularity.desc',
  })),
};

export const HOME_ROWS = [
  { category: 'trending', title: 'Trending Today', accentColor: '#f43f5e' },
  { category: 'new_releases', title: 'New Releases', accentColor: '#f97316' },
  { category: 'trending_shows', title: 'Trending Shows', accentColor: '#3b82f6' },
  { category: 'popular_movies', title: 'Popular Movies', accentColor: '#ec4899' },
  { category: 'airing_anime', title: 'Airing Anime', accentColor: '#a855f7' },
  { category: 'popular_anime', title: 'Popular Anime', accentColor: '#d376c3' },
  { category: 'top_movies', title: 'Top Rated Movies', accentColor: '#8b5cf6' },
  { category: 'top_shows', title: 'Top Rated Shows', accentColor: '#06b6d4' },
  { category: 'anime_movies', title: 'Anime Movies', accentColor: '#14b8a6' },
  { category: 'top_anime', title: 'Top Rated Anime', accentColor: '#eab308' },
];

// genre and keyword pages cover both movies and shows, merged by popularity
async function fetchTagList(category, filterId, type, pageNumber) {
  const params = { page: pageNumber, sort_by: 'popularity.desc', 'vote_count.gte': 10 };
  if (category === 'discover_by_genre') {
    params.with_genres = String(filterId ?? '');
  } else {
    params.with_keywords = String(filterId ?? '');
  }

  const types = type === 'movie' || type === 'tv' ? [type] : ['movie', 'tv'];
  const responses = await Promise.all(types.map((t) => tmdbGet(`discover/${t}`, params)));
  const results = responses
    .flatMap((response, i) => withType(response?.results, types[i]))
    .sort((a, b) => (b.popularity || 0) - (a.popularity || 0));
  return { results, total_pages: Math.max(...responses.map((r) => r?.total_pages || 1)) };
}

// a person's filmography, without talk show and news appearances or playing themselves
async function fetchPersonList(personId) {
  const credits = await getPersonCredits(personId);
  const noise = [10763, 10764, 10767];
  const seen = new Set();
  const results = [...(credits?.cast || []), ...(credits?.crew || [])]
    .filter((item) => item.media_type === 'movie' || item.media_type === 'tv')
    .filter((item) => item.poster_path && !genreIds(item).some((id) => noise.includes(id)))
    .filter((item) => !/\b(self|himself|herself|themselves)\b/i.test(item.character || ''))
    .filter((item) => {
      const key = `${item.id}-${item.media_type}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => (b.vote_count || 0) - (a.vote_count || 0));
  return { results, total_pages: 1 };
}

export async function fetchBrowseList({ category, type = 'all', filterId = null, page: pageNumber = 1 }) {
  if (category === 'discover_by_genre' || category === 'discover_by_keyword') {
    return fetchTagList(category, filterId, type, pageNumber);
  }
  if (category === 'person') {
    return fetchPersonList(filterId);
  }
  const list = LISTS[category];
  if (!list) throw new Error(`unknown list: ${category}`);
  return list(pageNumber);
}
