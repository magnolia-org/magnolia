//! anime numbering from anilist. tmdb often folds several cours into one season or
//! numbers a show straight through, while fansubs name each anilist entry ("2nd season")
//! and count its episodes from one, or count the whole run absolutely.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use crate::extensions::runtime::days_from_civil;
use crate::Core;

const ANILIST_URL: &str = "https://graphql.anilist.co";
const ANILIST_QUERY: &str = "query ($ids: [Int]) { Page(perPage: 50) { media(id_in: $ids, type: ANIME) { \
    id format episodes status title { romaji english } synonyms \
    startDate { year month day } endDate { year month day } \
    airingSchedule(perPage: 50) { nodes { episode airingAt } } } } }";
const DAY: i64 = 86_400;
/// shows still airing get new episodes and dates, finished ones don't
const AIRING_TTL: i64 = DAY;
const FINISHED_TTL: i64 = 30 * DAY;

/// one anilist entry: a season, a cour or the whole run of a show.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimeEntry {
    pub anilist_id: u32,
    pub format: String,
    pub episodes: Option<u32>,
    pub airing: bool,
    pub title: String,
    pub english_title: Option<String>,
    pub synonyms: Vec<String>,
    /// unix seconds
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// (episode, unix seconds it aired)
    pub schedule: Vec<(u32, i64)>,
}

/// where a tmdb episode falls in anilist's numbering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimeEpisode {
    pub anilist_id: u32,
    /// romaji title of the entry, e.g. "Sousou no Frieren 2nd Season"
    pub title: String,
    /// romaji title of the first entry, which fansubs name the show by
    pub franchise_title: String,
    /// latin-script names of the show, romaji first, for searches and title matching
    pub titles: Vec<String>,
    /// which entry of the show this is, counted from one
    pub season: u32,
    /// the episode inside that entry
    pub episode: u32,
    /// the episode counted from the start of the show
    pub absolute_episode: u32,
    pub episodes: Option<u32>,
}

/// a tmdb episode to place: its season, its number and when it aired.
#[derive(Debug, Clone, Default)]
pub struct TmdbEpisode {
    pub season: u32,
    pub episode: u32,
    /// episode counts of the regular seasons, by season number
    pub season_episode_counts: HashMap<u32, u32>,
    /// unix seconds
    pub air_date: Option<i64>,
}

impl TmdbEpisode {
    /// the episode counted from the first regular season.
    fn absolute(&self) -> u32 {
        let before: u32 = self
            .season_episode_counts
            .iter()
            .filter(|(s, _)| **s >= 1 && **s < self.season)
            .map(|(_, n)| *n)
            .sum();
        before + self.episode
    }
}

/// unix seconds for a "yyyy-mm-dd" date.
pub fn parse_ymd(date: &str) -> Option<i64> {
    let mut parts = date.trim().splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    Some(days_from_civil(y, m, d) * DAY)
}

fn is_latin(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii() || matches!(c, 'ā' | 'ē' | 'ī' | 'ō' | 'ū' | '’'))
}

/// places a tmdb episode in the show's anilist entries. episode counts walk the entries in
/// airing order; an air date, when known, overrides that, since tmdb and anilist sometimes
/// disagree on what counts as an episode.
pub fn map_episode(entries: &[AnimeEntry], tmdb: &TmdbEpisode) -> Option<AnimeEpisode> {
    let mut series: Vec<&AnimeEntry> = entries
        .iter()
        .filter(|e| matches!(e.format.as_str(), "TV" | "TV_SHORT" | "ONA"))
        .collect();
    series.sort_by_key(|e| e.start.unwrap_or(i64::MAX));
    let first = *series.first()?;

    let offset_of = |k: usize| -> Option<u32> { series[..k].iter().map(|e| e.episodes).sum() };
    let absolute = tmdb.absolute();

    // walk the entries by their episode counts
    let mut by_count = series.len() - 1;
    let mut offset = 0;
    for (k, entry) in series.iter().enumerate() {
        match entry.episodes {
            Some(n) if absolute > offset + n && k + 1 < series.len() => offset += n,
            _ => {
                by_count = k;
                break;
            }
        }
    }

    // the latest entry that had started by the air date
    let by_date = tmdb.air_date.and_then(|aired| {
        series
            .iter()
            .rposition(|e| e.start.is_some_and(|start| start <= aired + 2 * DAY))
    });
    let k = by_date.unwrap_or(by_count);
    let entry = series[k];

    let from_schedule = tmdb.air_date.and_then(|aired| {
        entry
            .schedule
            .iter()
            .filter(|(_, at)| (at - aired).abs() <= 2 * DAY)
            .min_by_key(|(_, at)| (at - aired).abs())
            .map(|(ep, _)| *ep)
    });
    let local = from_schedule
        .or_else(|| {
            let offset = offset_of(k)?;
            let local = absolute.checked_sub(offset).filter(|e| *e > 0)?;
            entry.episodes.map_or(true, |n| local <= n).then_some(local)
        })
        .unwrap_or(tmdb.episode);

    let mut titles = vec![first.title.clone()];
    for name in first.english_title.iter().chain(&first.synonyms).chain(std::iter::once(&entry.title)) {
        if is_latin(name) && !titles.iter().any(|t| t.eq_ignore_ascii_case(name)) {
            titles.push(name.clone());
        }
    }

    Some(AnimeEpisode {
        anilist_id: entry.anilist_id,
        title: entry.title.clone(),
        franchise_title: first.title.clone(),
        titles,
        season: k as u32 + 1,
        episode: local,
        absolute_episode: offset_of(k).map_or(absolute, |o| o + local),
        episodes: entry.episodes,
    })
}

/// a tmdb episode and when it aired, as the frontend lists them.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmdbEpisodeDate {
    pub season: u32,
    pub episode: u32,
    #[serde(default)]
    pub air_date: Option<String>,
}

/// a tmdb episode with its place in anilist's numbering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappedEpisode {
    pub season: u32,
    pub episode: u32,
    pub anime: AnimeEpisode,
}

/// anilist numbering for every episode of a show. tmdb splits some shows into segments
/// (panty & stocking airs two or three per broadcast), so several tmdb episodes can share
/// one anilist episode, and one file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShowNumbering {
    pub episodes: Vec<MappedEpisode>,
}

impl ShowNumbering {
    pub fn get(&self, season: u32, episode: u32) -> Option<&MappedEpisode> {
        self.episodes.iter().find(|m| m.season == season && m.episode == episode)
    }

    /// the tmdb episodes sharing one anilist episode with this one, in order. just the
    /// episode itself when it isn't split.
    pub fn group(&self, season: u32, episode: u32) -> Vec<(u32, u32)> {
        let Some(this) = self.get(season, episode) else {
            return vec![(season, episode)];
        };
        self.episodes
            .iter()
            .filter(|m| m.anime.anilist_id == this.anime.anilist_id && m.anime.episode == this.anime.episode)
            .map(|m| (m.season, m.episode))
            .collect()
    }

    /// every group of two or more tmdb episodes that air as one.
    pub fn split_groups(&self) -> Vec<Vec<(u32, u32)>> {
        let mut groups: Vec<Vec<(u32, u32)>> = Vec::new();
        for m in &self.episodes {
            let group = self.group(m.season, m.episode);
            if group.len() > 1 && group[0] == (m.season, m.episode) {
                groups.push(group);
            }
        }
        groups
    }
}

/// places every tmdb episode of a show in anilist's numbering. split episodes only group
/// when anilist's airing schedule ties them to one broadcast, so a season dropped in one
/// day (netflix) stays apart.
pub fn number_show(entries: &[AnimeEntry], episodes: &[TmdbEpisodeDate]) -> ShowNumbering {
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for e in episodes.iter().filter(|e| e.season > 0) {
        let count = counts.entry(e.season).or_default();
        *count = (*count).max(e.episode);
    }
    let episodes = episodes
        .iter()
        .filter(|e| e.season > 0 && e.episode > 0)
        .filter_map(|e| {
            let tmdb = TmdbEpisode {
                season: e.season,
                episode: e.episode,
                season_episode_counts: counts.clone(),
                air_date: e.air_date.as_deref().and_then(parse_ymd),
            };
            Some(MappedEpisode {
                season: e.season,
                episode: e.episode,
                anime: map_episode(entries, &tmdb)?,
            })
        })
        .collect();
    let mut numbering = ShowNumbering { episodes };
    split_same_day_broadcasts(entries, &mut numbering);
    numbering
}

/// when anilist aired several episodes on one day (a double finale), the tmdb segments of
/// that day all land on one of them; share them out in order instead.
fn split_same_day_broadcasts(entries: &[AnimeEntry], numbering: &mut ShowNumbering) {
    let mut start = 0;
    while start < numbering.episodes.len() {
        let first = &numbering.episodes[start];
        let mut end = start + 1;
        while end < numbering.episodes.len()
            && numbering.episodes[end].season == first.season
            && numbering.episodes[end].anime == first.anime
        {
            end += 1;
        }
        let run = end - start;
        let aired = entries
            .iter()
            .find(|e| e.anilist_id == first.anime.anilist_id)
            .and_then(|entry| {
                let at = entry.schedule.iter().find(|(ep, _)| *ep == first.anime.episode)?.1;
                let mut same_day: Vec<u32> = entry
                    .schedule
                    .iter()
                    .filter(|(_, t)| (t - at).abs() < DAY / 2)
                    .map(|(ep, _)| *ep)
                    .collect();
                same_day.sort_unstable();
                Some(same_day)
            })
            .unwrap_or_default();
        if aired.len() > 1 && run >= aired.len() {
            let base = numbering.episodes[start].anime.clone();
            let offset = base.absolute_episode as i64 - base.episode as i64;
            for i in 0..run {
                let ep = aired[i * aired.len() / run];
                let anime = &mut numbering.episodes[start + i].anime;
                anime.episode = ep;
                anime.absolute_episode = (offset + ep as i64) as u32;
            }
        }
        start = end;
    }
}

#[derive(Deserialize)]
struct FuzzyDate {
    year: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
}

impl FuzzyDate {
    fn unix(&self) -> Option<i64> {
        Some(days_from_civil(self.year?, self.month.unwrap_or(1), self.day.unwrap_or(1)) * DAY)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Media {
    id: u32,
    format: Option<String>,
    episodes: Option<u32>,
    status: Option<String>,
    title: MediaTitle,
    #[serde(default)]
    synonyms: Vec<String>,
    start_date: Option<FuzzyDate>,
    end_date: Option<FuzzyDate>,
    airing_schedule: Option<Schedule>,
}

#[derive(Deserialize)]
struct MediaTitle {
    romaji: Option<String>,
    english: Option<String>,
}

#[derive(Deserialize)]
struct Schedule {
    nodes: Vec<ScheduleNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleNode {
    episode: u32,
    airing_at: i64,
}

impl From<Media> for AnimeEntry {
    fn from(m: Media) -> Self {
        Self {
            anilist_id: m.id,
            format: m.format.unwrap_or_default(),
            episodes: m.episodes,
            airing: matches!(m.status.as_deref(), Some("RELEASING" | "NOT_YET_RELEASED")),
            title: m.title.romaji.or(m.title.english.clone()).unwrap_or_default(),
            english_title: m.title.english,
            synonyms: m.synonyms,
            start: m.start_date.and_then(|d| d.unix()),
            end: m.end_date.and_then(|d| d.unix()),
            schedule: m
                .airing_schedule
                .map(|s| s.nodes.into_iter().map(|n| (n.episode, n.airing_at)).collect())
                .unwrap_or_default(),
        }
    }
}

async fn fetch_entries(ids: &[u32]) -> Result<Vec<AnimeEntry>, String> {
    #[derive(Deserialize)]
    struct Response {
        data: Data,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Data {
        page: Page,
    }
    #[derive(Deserialize)]
    struct Page {
        media: Vec<Media>,
    }

    let body = serde_json::json!({ "query": ANILIST_QUERY, "variables": { "ids": ids } });
    let response = reqwest::Client::new()
        .post(ANILIST_URL)
        .json(&body)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| format!("anilist request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("anilist returned {}", response.status()));
    }
    let parsed: Response = response.json().await.map_err(|e| format!("anilist response: {}", e))?;
    Ok(parsed.data.page.media.into_iter().map(AnimeEntry::from).collect())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Serialize, Deserialize, Clone)]
struct CachedShow {
    fetched: i64,
    entries: Vec<AnimeEntry>,
}

/// anilist entries per tmdb show, and the numbering worked out from them, kept on disk.
pub struct AnimeDb {
    file: PathBuf,
    cache: Mutex<HashMap<u32, CachedShow>>,
    numbering_file: PathBuf,
    numbering: Mutex<HashMap<u32, ShowNumbering>>,
}

fn read_json<T: serde::de::DeserializeOwned + Default>(file: &PathBuf) -> T {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

impl AnimeDb {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let file = app_data_dir.join("anime_db.json");
        let numbering_file = app_data_dir.join("anime_numbering.json");
        Self {
            cache: Mutex::new(read_json(&file)),
            file,
            numbering: Mutex::new(read_json(&numbering_file)),
            numbering_file,
        }
    }

    /// the numbering last worked out for a show, used by file picking and watch tracking.
    pub fn numbering(&self, tmdb_id: u32) -> Option<ShowNumbering> {
        self.numbering.lock().ok()?.get(&tmdb_id).cloned()
    }

    fn store_numbering(&self, tmdb_id: u32, numbering: ShowNumbering) {
        let Ok(mut all) = self.numbering.lock() else { return };
        if all.get(&tmdb_id) == Some(&numbering) {
            return;
        }
        all.insert(tmdb_id, numbering);
        if let Ok(json) = serde_json::to_string(&*all) {
            if let Err(e) = std::fs::write(&self.numbering_file, json) {
                log::warn!("failed to save anime numbering: {}", e);
            }
        }
    }

    fn cached(&self, tmdb_id: u32) -> Option<Vec<AnimeEntry>> {
        let cache = self.cache.lock().ok()?;
        let show = cache.get(&tmdb_id)?;
        let ttl = if show.entries.iter().any(|e| e.airing) { AIRING_TTL } else { FINISHED_TTL };
        (now() - show.fetched < ttl).then(|| show.entries.clone())
    }

    fn store(&self, tmdb_id: u32, entries: Vec<AnimeEntry>) {
        let Ok(mut cache) = self.cache.lock() else { return };
        cache.insert(tmdb_id, CachedShow { fetched: now(), entries });
        if let Ok(json) = serde_json::to_string(&*cache) {
            if let Err(e) = std::fs::write(&self.file, json) {
                log::warn!("failed to save anime db: {}", e);
            }
        }
    }

    fn stale(&self, tmdb_id: u32) -> Option<Vec<AnimeEntry>> {
        Some(self.cache.lock().ok()?.get(&tmdb_id)?.entries.clone())
    }
}

impl Core {
    /// a show's anilist entries, or none when it isn't in the anime list or anilist can't
    /// be reached (an older copy is used then).
    async fn anime_entries(&self, tmdb_id: u32) -> Option<Vec<AnimeEntry>> {
        if let Some(entries) = self.anime_db.cached(tmdb_id) {
            return Some(entries);
        }
        let ids = self.anime_list.anilist_ids(tmdb_id).await;
        if ids.is_empty() {
            return None;
        }
        match fetch_entries(&ids).await {
            Ok(entries) => {
                self.anime_db.store(tmdb_id, entries.clone());
                Some(entries)
            }
            Err(e) => {
                log::warn!("anime numbering for tmdb {}: {}", tmdb_id, e);
                self.anime_db.stale(tmdb_id)
            }
        }
    }

    /// where a tmdb episode of an anime falls in anilist's numbering.
    pub async fn anime_episode(&self, tmdb_id: u32, tmdb: &TmdbEpisode) -> Option<AnimeEpisode> {
        map_episode(&self.anime_entries(tmdb_id).await?, tmdb)
    }

    /// the tmdb episodes that air as one with this one (split anime episodes share a file
    /// and a watch state), first to last. just the episode itself otherwise.
    pub fn episode_group(&self, media_id: u32, media_type: &str, season: u32, episode: u32) -> Vec<(u32, u32)> {
        match self.anime_db.numbering(media_id).filter(|_| media_type == "tv") {
            Some(numbering) => numbering.group(season, episode),
            None => vec![(season, episode)],
        }
    }

    /// the episode after this one, skipping the rest of a split anime episode's file. it may
    /// run past the end of the season, which the resume target resolves.
    pub fn next_episode(&self, media_id: u32, media_type: &str, season: u32, episode: u32) -> u32 {
        match self.episode_group(media_id, media_type, season, episode).last() {
            Some((s, e)) if *s == season => e + 1,
            _ => episode + 1,
        }
    }

    /// anilist numbering for every episode of an anime, saved so file picking and watch
    /// tracking can treat split episodes as one.
    pub async fn anime_numbering(&self, tmdb_id: u32, episodes: &[TmdbEpisodeDate]) -> Option<ShowNumbering> {
        let numbering = number_show(&self.anime_entries(tmdb_id).await?, episodes);
        if numbering.episodes.is_empty() {
            return None;
        }
        self.anime_db.store_numbering(tmdb_id, numbering.clone());
        Some(numbering)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, title: &str, episodes: Option<u32>, start: &str) -> AnimeEntry {
        AnimeEntry {
            anilist_id: id,
            format: "TV".into(),
            episodes,
            title: title.into(),
            start: parse_ymd(start),
            ..Default::default()
        }
    }

    fn frieren() -> Vec<AnimeEntry> {
        vec![
            entry(182255, "Sousou no Frieren 2nd Season", Some(10), "2026-01-16"),
            entry(154587, "Sousou no Frieren", Some(28), "2023-09-29"),
        ]
    }

    #[test]
    fn one_tmdb_season_spans_two_entries() {
        // tmdb lists all 38 episodes as season one
        let tmdb = TmdbEpisode {
            season: 1,
            episode: 33,
            season_episode_counts: HashMap::from([(1, 38)]),
            air_date: None,
        };
        let mapped = map_episode(&frieren(), &tmdb).unwrap();
        assert_eq!(mapped.title, "Sousou no Frieren 2nd Season");
        assert_eq!(mapped.franchise_title, "Sousou no Frieren");
        assert_eq!((mapped.season, mapped.episode, mapped.absolute_episode), (2, 5, 33));
    }

    #[test]
    fn tmdb_seasons_matching_entries() {
        let tmdb = TmdbEpisode {
            season: 2,
            episode: 5,
            season_episode_counts: HashMap::from([(1, 28), (2, 10)]),
            air_date: None,
        };
        let mapped = map_episode(&frieren(), &tmdb).unwrap();
        assert_eq!((mapped.season, mapped.episode, mapped.absolute_episode), (2, 5, 33));
    }

    #[test]
    fn air_dates_beat_episode_counts() {
        // tmdb counts a recap episode anilist doesn't, so counting alone lands one off
        let mut entries = frieren();
        entries[0].schedule = vec![(4, parse_ymd("2026-02-06").unwrap()), (5, parse_ymd("2026-02-13").unwrap())];
        let tmdb = TmdbEpisode {
            season: 1,
            episode: 34,
            season_episode_counts: HashMap::from([(1, 39)]),
            air_date: parse_ymd("2026-02-13"),
        };
        let mapped = map_episode(&entries, &tmdb).unwrap();
        assert_eq!((mapped.season, mapped.episode), (2, 5));
    }

    #[test]
    fn split_tmdb_episodes_group_by_broadcast() {
        // tmdb splits each broadcast into segments; anilist's schedule has one per date
        let mut new_ps = entry(1, "New PANTY & STOCKING with GARTERBELT", Some(13), "2025-07-12");
        new_ps.schedule = vec![(1, parse_ymd("2025-07-12").unwrap()), (2, parse_ymd("2025-07-19").unwrap())];
        let date = |d: &str| Some(d.to_string());
        let episodes = vec![
            TmdbEpisodeDate { season: 1, episode: 1, air_date: date("2025-07-12") },
            TmdbEpisodeDate { season: 1, episode: 2, air_date: date("2025-07-12") },
            TmdbEpisodeDate { season: 1, episode: 3, air_date: date("2025-07-19") },
            TmdbEpisodeDate { season: 1, episode: 4, air_date: date("2025-07-19") },
            TmdbEpisodeDate { season: 1, episode: 5, air_date: date("2025-07-19") },
        ];
        let numbering = number_show(&[new_ps], &episodes);
        assert_eq!(numbering.group(1, 4), vec![(1, 3), (1, 4), (1, 5)]);
        assert_eq!(numbering.get(1, 2).unwrap().anime.episode, 1);
        assert_eq!(numbering.split_groups(), vec![vec![(1, 1), (1, 2)], vec![(1, 3), (1, 4), (1, 5)]]);
    }

    #[test]
    fn same_day_drops_stay_apart() {
        // a whole season released at once has no schedule to tie episodes together
        let drop = entry(1, "Show", Some(3), "2025-01-01");
        let episodes: Vec<_> = (1..=3)
            .map(|e| TmdbEpisodeDate { season: 1, episode: e, air_date: Some("2025-01-01".into()) })
            .collect();
        let numbering = number_show(&[drop], &episodes);
        assert!(numbering.split_groups().is_empty());
    }

    #[test]
    fn single_long_entry_counts_absolutely() {
        let entries = vec![entry(21, "ONE PIECE", None, "1999-10-20")];
        let tmdb = TmdbEpisode {
            season: 3,
            episode: 4,
            season_episode_counts: HashMap::from([(1, 61), (2, 16), (3, 15)]),
            air_date: None,
        };
        let mapped = map_episode(&entries, &tmdb).unwrap();
        assert_eq!((mapped.season, mapped.episode, mapped.absolute_episode), (1, 81, 81));
    }
}
