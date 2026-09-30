pub mod magnet;
pub mod ranking;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::extensions::{runtime, LoadedExtension};
use crate::Core;
use ranking::RankContext;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub size: String,
    pub seeds: u32,
    pub peers: u32,
    pub magnet_link: String,
    pub provider: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub quality: Option<String>,
    pub encode: Option<String>,
    pub is_batch: bool,
    pub audio_codec: Option<String>,
    // ranking, filled in by `ranking::rank_results`
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub matches_episode: bool,
    #[serde(default)]
    pub has_release_year: bool,
    #[serde(default)]
    pub relevance: f64,
}

/// Metadata extracted from a release title. Used to auto-fill fields that
/// tracker extensions don't provide themselves.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TitleMeta {
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub quality: Option<String>,
    pub encode: Option<String>,
    pub is_batch: bool,
}

struct TitleRegexes {
    season: Regex,
    episode: Regex,
    quality: Regex,
    encode: Regex,
    batch: Regex,
}

fn title_regexes() -> &'static TitleRegexes {
    static REGEXES: OnceLock<TitleRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| TitleRegexes {
        season: Regex::new(r"(?i)S(\d{1,2})|Season\s*(\d{1,2})").unwrap(),
        episode: Regex::new(r"(?i)S\d{1,2}E(\d+)|E(\d+)|Episode\s*(\d+)|\s-\s*(\d+)\s*(?:v\d)?")
            .unwrap(),
        quality: Regex::new(r"(?i)(\d{3,4}p|4K|8K|2160p|1440p|1080p|720p|480p)").unwrap(),
        encode: Regex::new(r"(?i)(x264|x265|H\.?264|H\.?265|HEVC|AVC|VP9|AV1)").unwrap(),
        batch: Regex::new(r"(?i)(batch|complete|\d+-\d+|S\d+E\d+-E?\d+)").unwrap(),
    })
}

pub fn parse_title_metadata(title: &str) -> TitleMeta {
    let r = title_regexes();

    let season = r
        .season
        .captures(title)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .and_then(|m| m.as_str().parse().ok());

    let episode = r
        .episode
        .captures(title)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .or_else(|| c.get(4))
        })
        .and_then(|m| m.as_str().parse().ok());

    let quality = r
        .quality
        .captures(title)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_uppercase());

    let encode = r
        .encode
        .captures(title)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_uppercase());

    let mut is_batch = r.batch.is_match(title);
    if season.is_some() && (episode.is_none() || title.to_lowercase().contains("season")) {
        is_batch = true;
    }

    TitleMeta {
        season,
        episode,
        quality,
        encode,
        is_batch,
    }
}

pub fn parse_audio_codec(title: &str) -> Option<String> {
    let title_upper = title.to_uppercase();

    // Check for various audio codec patterns
    if title_upper.contains("FLAC") {
        Some("FLAC".to_string())
    } else if title_upper.contains("DTS-HD") || title_upper.contains("DTS-MA") {
        Some("DTS-HD".to_string())
    } else if title_upper.contains("DTS") {
        Some("DTS".to_string())
    } else if title_upper.contains("TRUEHD") || title_upper.contains("TRUE-HD") {
        Some("TrueHD".to_string())
    } else if title_upper.contains("DD+") || title_upper.contains("DDP") || title_upper.contains("E-AC-3") || title_upper.contains("EAC3") {
        Some("E-AC3".to_string())
    } else if title_upper.contains("AC3") || title_upper.contains("AC-3") || title_upper.contains("DD5.1") || title_upper.contains("DD 5.1") || title_upper.contains("DOLBY DIGITAL") {
        Some("AC3".to_string())
    } else if title_upper.contains("AAC") {
        Some("AAC".to_string())
    } else if title_upper.contains("OPUS") {
        Some("Opus".to_string())
    } else if title_upper.contains("VORBIS") {
        Some("Vorbis".to_string())
    } else if title_upper.contains("MP3") {
        Some("MP3".to_string())
    } else {
        None
    }
}

// Check if audio codec is supported by web browsers
// Based on: https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Audio_codecs
#[allow(dead_code)]
pub fn is_web_compatible(codec: Option<&str>) -> bool {
    match codec {
        // Supported web audio codecs
        Some("AAC") | Some("MP3") | Some("Opus") | Some("Vorbis") | Some("FLAC") => true,
        // Unsupported: AC3, E-AC3, DTS, DTS-HD, TrueHD
        Some("AC3") | Some("E-AC3") | Some("DTS") | Some("DTS-HD") | Some("TrueHD") => false,
        // Unknown codecs assumed incompatible
        None => true, // If no codec detected, don't filter out
        _ => false,
    }
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
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentSearchResponse {
    pub query: String,
    /// "anime", "movie" or "tv", which decides the trackers used in auto mode
    pub media_type: String,
    pub results: Vec<SearchResult>,
}

/// runs tracker extensions in parallel and collects their results.
async fn search_tracker_extensions(
    trackers: Vec<LoadedExtension>,
    query: &str,
    media_type: &str,
    imdb_id: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Vec<SearchResult> {
    let mut handles = vec![];

    for ext in trackers {
        let ctx = runtime::TrackerSearchContext {
            query: query.to_string(),
            media_type: Some(media_type.to_string()),
            imdb_id: imdb_id.clone(),
            season,
            episode,
        };
        // rhai execution and its http calls are blocking
        handles.push(tokio::task::spawn_blocking(move || {
            let result = runtime::run_tracker_search(&ext.source, &ext.manifest, &ext.field_values, &ctx);
            (ext.id, result)
        }));
    }

    let mut all_results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((id, Ok(results))) => {
                log::info!("tracker extension {} returned {} results", id, results.len());
                all_results.extend(results);
            }
            Ok((id, Err(e))) => log::warn!("tracker extension {} error: {}", id, e),
            Err(e) => log::warn!("tracker extension task panicked: {}", e),
        }
    }
    all_results
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

fn dedupe_by_info_hash(results: &mut Vec<SearchResult>) {
    let mut seen = HashSet::new();
    results.retain(|r| match magnet::extract_info_hash(&r.magnet_link) {
        Some(hash) => seen.insert(hash),
        None => true,
    });
}

impl Core {
    /// searches the chosen trackers (or, in auto mode, the anime or general
    /// ones), merges their results by info hash and ranks them for the request.
    pub async fn search_torrents(&self, req: TorrentSearchRequest) -> TorrentSearchResponse {
        let is_movie = req.media_type == "movie";
        let is_anime = match req.tmdb_id {
            Some(id) => {
                self.is_anime(id, Some(&req.media_type), &req.genre_ids, req.original_language.as_deref())
                    .await
            }
            None => looks_like_anime(&req.genre_ids, req.original_language.as_deref()),
        };
        let media_type = if is_anime { "anime" } else if is_movie { "movie" } else { "tv" };

        let query = req
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .unwrap_or(&req.title)
            .to_string();
        let normalized_query = query.replace(['-', ':', '_'], " ");

        let (season, episode) = if is_movie { (None, None) } else { (req.season, req.episode) };

        let preference = match req.trackers {
            Some(trackers) => trackers,
            None => self.settings.get().await.tracker_preference,
        };
        let is_auto = preference.is_empty();

        let all_trackers = self.extensions.enabled_trackers().await;
        let selected: Vec<LoadedExtension> = all_trackers
            .iter()
            .filter(|e| {
                if is_auto {
                    // anime goes to anime trackers, everything else to the general ones
                    e.manifest.is_anime == Some(is_anime)
                } else {
                    preference.contains(&e.id)
                }
            })
            .cloned()
            .collect();

        log::info!(
            "searching '{}' ({}) with trackers {:?}",
            normalized_query,
            media_type,
            selected.iter().map(|e| e.id.as_str()).collect::<Vec<_>>()
        );

        let mut results =
            search_tracker_extensions(selected, &normalized_query, media_type, req.imdb_id.clone(), season, episode).await;

        if is_auto && is_anime && results.len() < MIN_ANIME_RESULTS {
            log::info!(
                "anime search returned {} results, adding regular trackers",
                results.len()
            );
            let fallback: Vec<LoadedExtension> = all_trackers
                .iter()
                .filter(|e| e.manifest.is_anime == Some(false))
                .cloned()
                .collect();
            results.extend(
                search_tracker_extensions(fallback, &normalized_query, media_type, req.imdb_id.clone(), season, episode)
                    .await,
            );
        }

        let before = results.len();
        dedupe_by_info_hash(&mut results);
        log::info!("{} results ({} duplicates dropped)", results.len(), before - results.len());

        ranking::rank_results(
            &mut results,
            &RankContext {
                is_movie,
                is_tv: req.media_type == "tv",
                release_year: req.release_year,
                season: req.season,
                episode: req.episode,
            },
        );

        TorrentSearchResponse {
            query,
            media_type: media_type.to_string(),
            results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_anime;

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
}
