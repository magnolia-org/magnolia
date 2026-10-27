pub mod magnet;
pub mod ranking;
pub mod release;

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::anime_db::{self, AnimeEpisode, TmdbEpisode};
use crate::extensions::{runtime, LoadedExtension};
use crate::Core;
pub use ranking::MatchKind;
use ranking::RankContext;
pub use release::{parse_release, ReleaseInfo, Scope, Subtitles};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub size: String,
    #[serde(default)]
    pub size_bytes: u64,
    pub seeds: u32,
    pub peers: u32,
    pub magnet_link: String,
    #[serde(default)]
    pub info_hash: Option<String>,
    pub provider: String,
    /// every tracker that listed this torrent
    #[serde(default)]
    pub providers: Vec<String>,
    /// unix seconds
    #[serde(default)]
    pub uploaded: Option<i64>,
    #[serde(default)]
    pub file_count: Option<u32>,
    /// uploaded by an account the tracker vouches for
    #[serde(default)]
    pub trusted: bool,
    #[serde(flatten)]
    pub release: ReleaseInfo,
    // ranking, filled in by `ranking::rank_result`
    #[serde(default)]
    pub match_kind: MatchKind,
    #[serde(default)]
    pub matches_episode: bool,
    #[serde(default)]
    pub has_release_year: bool,
    #[serde(default)]
    pub title_score: f64,
    #[serde(default)]
    pub relevance: f64,
}

impl SearchResult {
    /// what duplicates from different trackers share: the info hash, else the magnet.
    pub fn key(&self) -> String {
        self.info_hash
            .clone()
            .or_else(|| magnet::extract_info_hash(&self.magnet_link))
            .unwrap_or_else(|| self.magnet_link.clone())
    }

    /// folds a duplicate listing into this one. true when anything changed.
    fn merge(&mut self, other: SearchResult) -> bool {
        let mut changed = false;
        for provider in other.providers {
            if !self.providers.contains(&provider) {
                self.providers.push(provider);
                changed = true;
            }
        }
        if other.seeds > self.seeds {
            self.seeds = other.seeds;
            changed = true;
        }
        if other.peers > self.peers {
            self.peers = other.peers;
            changed = true;
        }
        if self.size_bytes == 0 && other.size_bytes > 0 {
            self.size = other.size;
            self.size_bytes = other.size_bytes;
            changed = true;
        }
        if self.uploaded.is_none() && other.uploaded.is_some() {
            self.uploaded = other.uploaded;
            changed = true;
        }
        if self.file_count.is_none() && other.file_count.is_some() {
            self.file_count = other.file_count;
            changed = true;
        }
        if self.release.subtitles.is_none() && other.release.subtitles.is_some() {
            self.release.subtitles = other.release.subtitles;
            changed = true;
        }
        if other.trusted && !self.trusted {
            self.trusted = true;
            changed = true;
        }
        changed
    }
}

/// the kind of query a search pass runs. trackers can opt out of kinds they
/// can't answer (eztv looks shows up by imdb id, so one pass is enough).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassKind {
    /// "show s01e05"
    Episode,
    /// "show s01", for season packs
    Season,
    /// the bare show name, for complete series and oddly named packs
    Show,
    Movie,
}

impl PassKind {
    pub const ALL: [PassKind; 4] = [PassKind::Episode, PassKind::Season, PassKind::Show, PassKind::Movie];

    pub fn as_str(self) -> &'static str {
        match self {
            PassKind::Episode => "episode",
            PassKind::Season => "season",
            PassKind::Show => "show",
            PassKind::Movie => "movie",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPass {
    pub kind: PassKind,
    pub query: String,
}

/// the queries one search runs. shows get an episode, a season and a bare name
/// query, which together find single episodes, season packs and whole series.
pub fn build_passes(
    base: &str,
    is_movie: bool,
    year: Option<u32>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Vec<SearchPass> {
    let pass = |kind, query: String| SearchPass { kind, query };
    if is_movie {
        return vec![match year {
            Some(year) => pass(PassKind::Movie, format!("{} {}", base, year)),
            None => pass(PassKind::Movie, base.to_string()),
        }];
    }
    // a query that already names a season or episode is run as typed
    let typed = parse_release(base);
    if typed.season.is_some() || typed.episode.is_some() {
        return vec![pass(PassKind::Show, base.to_string())];
    }
    let mut passes = Vec::new();
    if let Some(s) = season.filter(|s| *s > 0) {
        if let Some(e) = episode.filter(|e| *e > 0) {
            passes.push(pass(PassKind::Episode, format!("{} S{:02}E{:02}", base, s, e)));
        }
        passes.push(pass(PassKind::Season, format!("{} S{:02}", base, s)));
    }
    passes.push(pass(PassKind::Show, base.to_string()));
    passes
}

/// queries in anilist's numbering, for anime trackers: fansubs name the show in romaji and
/// number episodes within each entry ("sousou no frieren 2nd season 05").
pub fn build_anime_passes(anime: &AnimeEpisode) -> Vec<SearchPass> {
    let franchise = normalize_query(&anime.franchise_title);
    let entry = normalize_query(&anime.title);
    vec![
        SearchPass {
            kind: PassKind::Episode,
            query: format!("{} {:02}", entry, anime.episode),
        },
        SearchPass {
            kind: PassKind::Season,
            query: entry,
        },
        SearchPass {
            kind: PassKind::Show,
            query: franchise,
        },
    ]
}

/// a search query with punctuation trackers trip over turned into spaces.
pub fn normalize_query(query: &str) -> String {
    query
        .replace(['-', ':', '_', '/'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// a torrent search for one movie or episode.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSearchRequest {
    #[serde(default)]
    pub tmdb_id: Option<u32>,
    /// "movie" or "tv"
    pub media_type: String,
    pub title: String,
    /// what the user typed instead of the title, if anything
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub genre_ids: Vec<u32>,
    #[serde(default)]
    pub release_year: Option<u32>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    /// for trackers that look shows up by imdb id
    #[serde(default)]
    pub imdb_id: Option<String>,
    /// tracker extension ids, empty for auto, none for the saved preference
    #[serde(default)]
    pub trackers: Option<Vec<String>>,
    /// tmdb's original_language, which tells japanese anime from western cartoons
    #[serde(default)]
    pub original_language: Option<String>,
    /// episode counts of the show's regular seasons, for anime numbering
    #[serde(default)]
    pub season_episode_counts: HashMap<u32, u32>,
    /// "yyyy-mm-dd", for anime numbering
    #[serde(default)]
    pub episode_air_date: Option<String>,
}

/// one tracker running one query.
#[derive(Debug, Clone, Serialize)]
pub struct SearchJob {
    pub tracker_id: String,
    pub tracker: String,
    pub kind: PassKind,
    pub query: String,
}

/// progress of a streaming search, in the order it happens.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchEvent {
    Started {
        search_id: u64,
        query: String,
        /// "anime", "movie" or "tv", which decides the trackers used in auto mode
        media_type: String,
        /// where the episode falls in anilist's numbering, for anime
        anime: Option<AnimeEpisode>,
        jobs: Vec<SearchJob>,
    },
    /// more jobs, when the first ones found too little
    JobsAdded {
        search_id: u64,
        reason: String,
        jobs: Vec<SearchJob>,
    },
    /// new results, or known ones (same `key`) another tracker added to
    Results {
        search_id: u64,
        results: Vec<SearchResult>,
    },
    JobFinished {
        search_id: u64,
        job: SearchJob,
        count: usize,
        error: Option<String>,
        elapsed_ms: u64,
        cached: bool,
    },
    Finished {
        search_id: u64,
        total: usize,
        elapsed_ms: u64,
        cancelled: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentSearchResponse {
    pub search_id: u64,
    pub query: String,
    pub media_type: String,
    pub anime: Option<AnimeEpisode>,
    /// best first
    pub results: Vec<SearchResult>,
}

/// a slow tracker is reported as failed after this, so it can't hold the search up.
const TRACKER_TIMEOUT: Duration = Duration::from_secs(20);
/// tracker answers are reused for this long, so reopening a search is instant.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_LIMIT: usize = 200;

/// search cancellation and the tracker answer cache.
pub struct SearchService {
    generation: watch::Sender<u64>,
    cache: Mutex<HashMap<String, (Instant, Vec<SearchResult>)>>,
}

impl Default for SearchService {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchService {
    pub fn new() -> Self {
        Self {
            generation: watch::Sender::new(0),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// stops whatever search is running.
    pub fn cancel(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    fn begin(&self) -> (u64, watch::Receiver<u64>) {
        self.cancel();
        let rx = self.generation.subscribe();
        let id = *rx.borrow();
        (id, rx)
    }

    fn cached(&self, key: &str) -> Option<Vec<SearchResult>> {
        let cache = self.cache.lock().ok()?;
        let (at, results) = cache.get(key)?;
        (at.elapsed() < CACHE_TTL).then(|| results.clone())
    }

    fn store(&self, key: String, results: Vec<SearchResult>) {
        let Ok(mut cache) = self.cache.lock() else { return };
        if cache.len() >= CACHE_LIMIT {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), results));
    }

    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

fn cache_key(ext: &LoadedExtension, ctx: &runtime::TrackerSearchContext) -> String {
    let mut hasher = DefaultHasher::new();
    ext.source.hash(&mut hasher);
    let mut fields: Vec<_> = ext.field_values.iter().collect();
    fields.sort();
    fields.hash(&mut hasher);
    format!(
        "{}|{:x}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}",
        ext.id,
        hasher.finish(),
        ctx.kind,
        ctx.query,
        ctx.media_type,
        ctx.imdb_id,
        ctx.season,
        ctx.episode,
        ctx.anime.as_ref().map(|a| (a.anilist_id, a.episode))
    )
}

struct Job {
    ext: LoadedExtension,
    ctx: runtime::TrackerSearchContext,
    info: SearchJob,
}

struct JobOutcome {
    info: SearchJob,
    result: Result<Vec<SearchResult>, String>,
    cached: bool,
    elapsed_ms: u64,
}

type JobFuture = Pin<Box<dyn Future<Output = JobOutcome> + Send>>;

/// runs one tracker query, from the cache when it can. a timed out query keeps
/// running and still fills the cache, so trying again later is fast.
fn run_job(service: Arc<SearchService>, job: Job) -> JobFuture {
    Box::pin(async move {
        let started = Instant::now();
        let key = cache_key(&job.ext, &job.ctx);
        if let Some(results) = service.cached(&key) {
            return JobOutcome {
                info: job.info,
                result: Ok(results),
                cached: true,
                elapsed_ms: 0,
            };
        }
        let Job { ext, ctx, info } = job;
        // rhai execution and its http calls are blocking
        let handle = tokio::task::spawn_blocking(move || {
            let result = runtime::run_tracker_search(&ext.source, &ext.manifest, &ext.field_values, &ctx);
            if let Ok(results) = &result {
                service.store(key, results.clone());
            }
            result
        });
        let result = match tokio::time::timeout(TRACKER_TIMEOUT, handle).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(format!("tracker crashed: {}", e)),
            Err(_) => Err(format!("timed out after {}s", TRACKER_TIMEOUT.as_secs())),
        };
        JobOutcome {
            info,
            result,
            cached: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }
    })
}

/// a set of jobs that only runs while the search has fewer results than `min_results`.
struct Stage {
    jobs: Vec<Job>,
    min_results: usize,
    reason: &'static str,
}

/// tmdb calls every cartoon "animation" (genre 16), but anime trackers only carry japanese
/// animation; south park or arcane searched there turns up nothing useful. without a known
/// language the genre alone decides, as before.
pub fn looks_like_anime(genre_ids: &[u32], original_language: Option<&str>) -> bool {
    genre_ids.contains(&16) && original_language.map_or(true, |lang| lang == "ja")
}

/// anime trackers returning fewer results than this also get the general trackers searched,
/// so one stray hit (a dvd rip of the movie) can't hide every real episode release.
const MIN_ANIME_RESULTS: usize = 5;
/// a movie search by name and year finding fewer than this also searches the bare name.
const MIN_MOVIE_RESULTS: usize = 10;
/// anilist answers in well under a second; past this the search goes ahead without it.
const ANIME_LOOKUP_TIMEOUT: Duration = Duration::from_secs(4);

impl Core {
    /// stops the running search; its remaining results are dropped.
    pub fn cancel_torrent_search(&self) {
        self.search.cancel();
    }

    /// `search_torrents_streaming` without the progress events.
    pub async fn search_torrents(&self, req: TorrentSearchRequest) -> TorrentSearchResponse {
        self.search_torrents_streaming(req, |_| {}).await
    }

    /// searches the chosen trackers (or, in auto mode, the anime or general ones)
    /// with every pass at once. results are ranked and merged by info hash as each
    /// tracker answers, and handed to `emit` straight away. starting a search
    /// cancels the one before it.
    pub async fn search_torrents_streaming<F>(&self, req: TorrentSearchRequest, emit: F) -> TorrentSearchResponse
    where
        F: Fn(SearchEvent) + Send + Sync,
    {
        let started = Instant::now();
        let (search_id, mut generation) = self.search.begin();

        let is_movie = req.media_type == "movie";
        let is_anime = match req.tmdb_id {
            Some(id) => {
                self.is_anime(id, Some(&req.media_type), &req.genre_ids, req.original_language.as_deref())
                    .await
            }
            None => looks_like_anime(&req.genre_ids, req.original_language.as_deref()),
        };
        let media_type = if is_anime { "anime" } else if is_movie { "movie" } else { "tv" };

        // re-searching sends the shown query back, which is only custom once it differs from the title
        let custom_query = req
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty() && normalize_query(q).to_lowercase() != normalize_query(&req.title).to_lowercase());
        let query = custom_query.unwrap_or(&req.title).to_string();
        let base = normalize_query(&query);
        let (season, episode) = if is_movie { (None, None) } else { (req.season, req.episode) };
        let year = if custom_query.is_some() { None } else { req.release_year };
        let western_passes = build_passes(&base, is_movie, year, season, episode);

        let anime = match (is_anime && !is_movie, req.tmdb_id, season, episode) {
            (true, Some(tmdb_id), Some(s), Some(e)) if s > 0 && e > 0 => {
                let tmdb = TmdbEpisode {
                    season: s,
                    episode: e,
                    season_episode_counts: req.season_episode_counts.clone(),
                    air_date: req.episode_air_date.as_deref().and_then(anime_db::parse_ymd),
                };
                tokio::time::timeout(ANIME_LOOKUP_TIMEOUT, self.anime_episode(tmdb_id, &tmdb))
                    .await
                    .ok()
                    .flatten()
            }
            _ => None,
        };
        // a typed query is run as typed
        let anime_passes = anime.as_ref().filter(|_| custom_query.is_none()).map(build_anime_passes);
        let passes = anime_passes.clone().unwrap_or_else(|| western_passes.clone());
        if let Some(a) = &anime {
            log::info!(
                "anime numbering: tmdb s{:?}e{:?} is '{}' episode {} (absolute {})",
                season,
                episode,
                a.title,
                a.episode,
                a.absolute_episode
            );
        }

        let preference = match req.trackers.clone() {
            Some(trackers) => trackers,
            None => self.settings.get().await.tracker_preference,
        };
        let is_auto = preference.is_empty();
        let all_trackers = self.extensions.enabled_trackers().await;
        let pick = |filter: &dyn Fn(&LoadedExtension) -> bool| -> Vec<LoadedExtension> {
            all_trackers.iter().filter(|e| filter(e)).cloned().collect()
        };
        let selected = if is_auto {
            // anime goes to anime trackers, everything else to the general ones
            pick(&|e| e.manifest.is_anime == Some(is_anime))
        } else {
            pick(&|e| preference.contains(&e.id))
        };

        let make_jobs = |trackers: &[LoadedExtension], passes: &[SearchPass]| -> Vec<Job> {
            let mut jobs = Vec::new();
            for ext in trackers {
                for pass in passes.iter().filter(|p| ext.manifest.supports_kind(p.kind)) {
                    jobs.push(Job {
                        ext: ext.clone(),
                        ctx: runtime::TrackerSearchContext {
                            query: pass.query.clone(),
                            kind: pass.kind.as_str().to_string(),
                            title: base.clone(),
                            anime: anime.clone(),
                            year: req.release_year,
                            media_type: Some(media_type.to_string()),
                            imdb_id: req.imdb_id.clone(),
                            season,
                            episode,
                        },
                        info: SearchJob {
                            tracker_id: ext.id.clone(),
                            tracker: ext.manifest.tracker_name.clone().unwrap_or_else(|| ext.manifest.name.clone()),
                            kind: pass.kind,
                            query: pass.query.clone(),
                        },
                    });
                }
            }
            jobs
        };

        let mut stages = vec![Stage {
            jobs: make_jobs(&selected, &passes),
            min_results: usize::MAX,
            reason: "",
        }];
        if is_movie && year.is_some() {
            stages.push(Stage {
                jobs: make_jobs(&selected, &build_passes(&base, true, None, None, None)),
                min_results: MIN_MOVIE_RESULTS,
                reason: "few results with the release year, searching the name alone",
            });
        }
        if is_auto && is_anime {
            let general = pick(&|e| e.manifest.is_anime == Some(false));
            stages.push(Stage {
                jobs: make_jobs(&general, &western_passes),
                min_results: MIN_ANIME_RESULTS,
                reason: "few anime results, adding the general trackers",
            });
        }

        let rank_ctx = RankContext {
            is_movie,
            is_anime,
            title: release::normalize_name(&base),
            release_year: req.release_year,
            season,
            episode,
            alt_titles: anime
                .iter()
                .flat_map(|a| a.titles.iter())
                .map(|t| release::normalize_name(t))
                .collect(),
            anime: anime.clone(),
        };

        log::info!(
            "search {} for '{}' ({}): {} passes on {:?}",
            search_id,
            base,
            media_type,
            passes.len(),
            selected.iter().map(|e| e.id.as_str()).collect::<Vec<_>>()
        );

        let mut merged: HashMap<String, SearchResult> = HashMap::new();
        let mut pending: FuturesUnordered<JobFuture> = FuturesUnordered::new();
        let mut stages = stages.into_iter();
        let mut cancelled = false;
        let mut first = true;

        loop {
            if pending.is_empty() {
                let Some(stage) = stages.by_ref().find(|s| merged.len() < s.min_results && !s.jobs.is_empty())
                else {
                    break;
                };
                let infos: Vec<SearchJob> = stage.jobs.iter().map(|j| j.info.clone()).collect();
                if first {
                    emit(SearchEvent::Started {
                        search_id,
                        query: query.clone(),
                        media_type: media_type.to_string(),
                        anime: anime.clone(),
                        jobs: infos,
                    });
                    first = false;
                } else {
                    log::info!("search {}: {}", search_id, stage.reason);
                    emit(SearchEvent::JobsAdded {
                        search_id,
                        reason: stage.reason.to_string(),
                        jobs: infos,
                    });
                }
                for job in stage.jobs {
                    pending.push(run_job(self.search.clone(), job));
                }
                continue;
            }

            let outcome = tokio::select! {
                Some(outcome) = pending.next() => outcome,
                _ = generation.changed() => {
                    cancelled = true;
                    break;
                }
            };

            let mut changed = Vec::new();
            let (count, error) = match outcome.result {
                Ok(results) => {
                    let count = results.len();
                    for mut r in results {
                        rank_result_into(&mut r, &rank_ctx);
                        match merged.entry(r.key()) {
                            Entry::Occupied(mut existing) => {
                                if existing.get_mut().merge(r) {
                                    ranking::rank_result(existing.get_mut(), &rank_ctx);
                                    changed.push(existing.get().clone());
                                }
                            }
                            Entry::Vacant(slot) => {
                                changed.push(r.clone());
                                slot.insert(r);
                            }
                        }
                    }
                    (count, None)
                }
                Err(e) => {
                    log::warn!("tracker {} ({}) failed: {}", outcome.info.tracker_id, outcome.info.query, e);
                    (0, Some(e))
                }
            };
            log::info!(
                "search {}: {} '{}' gave {} results in {} ms{}",
                search_id,
                outcome.info.tracker_id,
                outcome.info.query,
                count,
                outcome.elapsed_ms,
                if outcome.cached { " (cached)" } else { "" }
            );
            if !changed.is_empty() {
                emit(SearchEvent::Results { search_id, results: changed });
            }
            emit(SearchEvent::JobFinished {
                search_id,
                job: outcome.info,
                count,
                error,
                elapsed_ms: outcome.elapsed_ms,
                cached: outcome.cached,
            });
        }

        if first {
            // no tracker could take any of the passes
            emit(SearchEvent::Started {
                search_id,
                query: query.clone(),
                media_type: media_type.to_string(),
                anime: anime.clone(),
                jobs: Vec::new(),
            });
        }

        let mut results: Vec<SearchResult> = merged.into_values().collect();
        results.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
        let elapsed_ms = started.elapsed().as_millis() as u64;
        log::info!(
            "search {} done: {} results in {} ms{}",
            search_id,
            results.len(),
            elapsed_ms,
            if cancelled { " (cancelled)" } else { "" }
        );
        emit(SearchEvent::Finished {
            search_id,
            total: results.len(),
            elapsed_ms,
            cancelled,
        });

        TorrentSearchResponse {
            search_id,
            query,
            media_type: media_type.to_string(),
            anime,
            results,
        }
    }
}

/// a fresh result lists its own tracker as its first provider before ranking.
fn rank_result_into(r: &mut SearchResult, ctx: &RankContext) {
    if r.providers.is_empty() {
        r.providers.push(r.provider.clone());
    }
    ranking::rank_result(r, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn western_animation_is_not_anime() {
        // south park, the simpsons, arcane: animation, but not for anime trackers
        assert!(!looks_like_anime(&[16, 35], Some("en")));
        assert!(!looks_like_anime(&[16, 10759], Some("fr")));
    }

    #[test]
    fn japanese_animation_is_anime() {
        assert!(looks_like_anime(&[16, 10759, 10765], Some("ja")));
    }

    #[test]
    fn unknown_language_falls_back_to_genre() {
        assert!(looks_like_anime(&[16], None));
        assert!(!looks_like_anime(&[18], None));
    }

    #[test]
    fn live_action_is_never_anime() {
        assert!(!looks_like_anime(&[18, 80], Some("ja")));
    }

    #[test]
    fn shows_search_episode_season_and_name() {
        let passes = build_passes("Breaking Bad", false, Some(2008), Some(1), Some(5));
        let queries: Vec<_> = passes.iter().map(|p| (p.kind, p.query.as_str())).collect();
        assert_eq!(
            queries,
            vec![
                (PassKind::Episode, "Breaking Bad S01E05"),
                (PassKind::Season, "Breaking Bad S01"),
                (PassKind::Show, "Breaking Bad"),
            ]
        );
    }

    #[test]
    fn movies_search_with_their_year() {
        let passes = build_passes("Dune", true, Some(2021), None, None);
        assert_eq!(passes, vec![SearchPass { kind: PassKind::Movie, query: "Dune 2021".into() }]);
    }

    #[test]
    fn typed_episode_queries_run_as_typed() {
        let passes = build_passes("Breaking Bad S02", false, None, Some(1), Some(5));
        assert_eq!(passes, vec![SearchPass { kind: PassKind::Show, query: "Breaking Bad S02".into() }]);
    }

    #[test]
    fn queries_drop_punctuation() {
        assert_eq!(normalize_query("Frieren: Beyond Journey's End"), "Frieren Beyond Journey's End");
    }

    #[test]
    fn duplicates_merge_providers_and_seeds() {
        let mut a = SearchResult {
            seeds: 5,
            providers: vec!["A".into()],
            ..Default::default()
        };
        let b = SearchResult {
            seeds: 9,
            providers: vec!["B".into()],
            ..Default::default()
        };
        assert!(a.merge(b));
        assert_eq!(a.seeds, 9);
        assert_eq!(a.providers, vec!["A", "B"]);
    }
}
