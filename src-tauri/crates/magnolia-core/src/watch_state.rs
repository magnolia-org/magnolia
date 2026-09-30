//! rules deciding what counts as watched, finished or resumable.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::watch_history::WatchHistoryItem;

// an entry counts as watched once playback is near the end or the player saw it finish
const WATCHED_RATIO: f64 = 0.9;
const WATCHED_REMAINING_SECONDS: f64 = 180.0;
// reaching the credits counts as finished even if the viewer closes before the very end
const CREDITS_COMPLETION_RATIO: f64 = 0.7;
// mpv also reports end-file on stream errors, so only trust it near the end
const END_FILE_COMPLETION_RATIO: f64 = 0.8;
// resuming a movie within its first minute isn't worth offering
const MIN_MOVIE_RESUME_SECONDS: f64 = 60.0;

const SKIPPABLE_CHAPTER_MARKERS: &[&str] = &["intro", "op", "opening", "recap", "re-cap", "eyecatch"];

pub fn progress_key(media_id: u32, media_type: &str) -> String {
    format!("{}-{}", media_id, media_type)
}

pub fn episode_key(media_id: u32, media_type: &str, season: u32, episode: u32) -> String {
    format!("{}-{}-S{}-E{}", media_id, media_type, season, episode)
}

/// one saved playback position, as stored in `watch_progress.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_timestamp: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_episode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

impl ProgressEntry {
    /// entries were written by older frontends too, so anything unreadable is treated as absent.
    pub fn from_value(value: &Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }

    pub fn timestamp(&self) -> f64 {
        self.current_timestamp.filter(|t| t.is_finite()).unwrap_or(0.0)
    }

    fn duration(&self) -> f64 {
        self.duration.filter(|d| d.is_finite()).unwrap_or(0.0)
    }

    pub fn episode(&self) -> Option<(u32, u32)> {
        match (self.current_season, self.current_episode) {
            (Some(s), Some(e)) if s > 0 && e > 0 => Some((s, e)),
            _ => None,
        }
    }

    pub fn watch_ratio(&self) -> f64 {
        let duration = self.duration();
        if duration <= 0.0 {
            return 0.0;
        }
        (self.timestamp() / duration).clamp(0.0, 1.0)
    }

    pub fn is_watched(&self) -> bool {
        if self.completed == Some(true) {
            return true;
        }
        let duration = self.duration();
        if duration <= 0.0 {
            return false;
        }
        let ratio = self.watch_ratio();
        let remaining = duration - self.timestamp();
        ratio >= WATCHED_RATIO || (ratio >= 0.5 && remaining <= WATCHED_REMAINING_SECONDS)
    }
}

pub fn is_value_watched(value: Option<&Value>) -> bool {
    value
        .and_then(ProgressEntry::from_value)
        .map(|e| e.is_watched())
        .unwrap_or(false)
}

/// copy of a stored entry with its `watched` flag filled in for the ui.
pub fn with_watched_flag(value: &Value) -> Value {
    let mut out = value.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("watched".to_string(), Value::Bool(is_value_watched(Some(value))));
    }
    out
}

// the progress entry that decides whether the whole title is done, or none if it can't be
fn finishing_entry(item: &WatchHistoryItem, progress: &HashMap<String, Value>) -> Option<ProgressEntry> {
    let key = progress_key(item.id, &item.media_type);
    let entry = ProgressEntry::from_value(progress.get(&key)?)?;
    if item.media_type != "tv" {
        return Some(entry);
    }

    let (season, episode) = entry.episode()?;

    // without the series length a finale looks like any other episode
    let last_season = item.last_season_number.or(item.number_of_seasons).filter(|n| *n > 0)?;
    let last_episode_count = item.last_season_episode_count.filter(|n| *n > 0)?;
    if season != last_season || episode < last_episode_count {
        return None;
    }

    progress
        .get(&episode_key(item.id, &item.media_type, season, episode))
        .and_then(ProgressEntry::from_value)
        .or(Some(entry))
}

pub fn is_media_finished(item: &WatchHistoryItem, progress: &HashMap<String, Value>) -> bool {
    finishing_entry(item, progress).map(|e| e.is_watched()).unwrap_or(false)
}

/// a history item plus whether the whole title has been watched.
#[derive(Debug, Clone, Serialize)]
pub struct WatchHistoryEntry {
    #[serde(flatten)]
    pub item: WatchHistoryItem,
    pub finished: bool,
}

/// unfinished titles by when they were last watched, then finished titles by when they were finished.
pub fn sort_watch_history(history: Vec<WatchHistoryItem>, progress: &HashMap<String, Value>) -> Vec<WatchHistoryEntry> {
    let mut unfinished = Vec::new();
    let mut finished = Vec::new();
    for item in history {
        let updated_at = progress
            .get(&progress_key(item.id, &item.media_type))
            .and_then(ProgressEntry::from_value)
            .and_then(|e| e.updated_at)
            .unwrap_or(0);
        let last_watched = item.watched_at.max(updated_at);
        match finishing_entry(&item, progress).filter(|e| e.is_watched()) {
            Some(done) => {
                let time = done
                    .completed_at
                    .filter(|t| *t > 0)
                    .or(done.updated_at.filter(|t| *t > 0))
                    .unwrap_or(last_watched);
                finished.push((time, WatchHistoryEntry { item, finished: true }));
            }
            None => unfinished.push((last_watched, WatchHistoryEntry { item, finished: false })),
        }
    }
    unfinished.sort_by(|a, b| b.0.cmp(&a.0));
    finished.sort_by(|a, b| b.0.cmp(&a.0));
    unfinished.into_iter().chain(finished).map(|(_, e)| e).collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonInfo {
    pub season_number: u32,
    #[serde(default)]
    pub episode_count: u32,
}

/// where playback should continue from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResumeTarget {
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub timestamp: f64,
}

/// movies resume mid-film; shows resume the current episode, or move past it
/// once it's watched. none when there's nothing left to resume.
pub fn resume_target(is_movie: bool, entry: Option<&ProgressEntry>, seasons: &[SeasonInfo]) -> Option<ResumeTarget> {
    let entry = entry?;

    if is_movie {
        let timestamp = entry.timestamp();
        if timestamp > MIN_MOVIE_RESUME_SECONDS && !entry.is_watched() {
            return Some(ResumeTarget { season: None, episode: None, timestamp });
        }
        return None;
    }

    let (mut season, mut episode) = entry.episode()?;
    let mut timestamp = entry.timestamp();

    if let Some(info) = seasons.iter().find(|s| s.season_number == season) {
        // "next episode" from the player can point one past the end of a season
        let past_season_end = episode > info.episode_count;
        if entry.is_watched() || past_season_end {
            if episode < info.episode_count {
                episode += 1;
            } else {
                // nothing left to resume once the last episode is done
                if !seasons.iter().any(|s| s.season_number == season + 1) {
                    return None;
                }
                season += 1;
                episode = 1;
            }
            timestamp = 0.0;
        }
    }

    Some(ResumeTarget {
        season: Some(season),
        episode: Some(episode),
        timestamp,
    })
}

/// where a fresh player should start: the saved position when it belongs to
/// this movie or episode, otherwise the beginning. a finished position starts
/// over instead of resuming into the credits.
pub fn initial_timestamp(entry: Option<&ProgressEntry>, is_movie: bool, season: Option<u32>, episode: Option<u32>) -> f64 {
    let Some(entry) = entry else { return 0.0 };
    if entry.is_watched() {
        return 0.0;
    }
    if is_movie {
        return entry.timestamp();
    }
    match (season, episode) {
        (Some(s), Some(e)) if entry.current_season == Some(s) && entry.current_episode == Some(e) => entry.timestamp(),
        _ => 0.0,
    }
}

pub fn is_ending_chapter(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("ending") || (t.contains("credits") && !t.contains("opening")) || t == "end"
}

pub fn is_skippable_chapter(title: &str) -> bool {
    let t = title.to_lowercase();
    SKIPPABLE_CHAPTER_MARKERS.iter().any(|m| t.contains(m))
}

/// watches one file's playback and reports, once, when it should count as watched.
#[derive(Debug, Default)]
pub struct CompletionTracker {
    // (start time, is ending) per chapter, in playback order
    chapters: Vec<(f64, bool)>,
    completed: bool,
}

impl CompletionTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn set_chapters(&mut self, chapters: Vec<(f64, String)>) {
        self.chapters = chapters
            .into_iter()
            .map(|(start, title)| (start, is_ending_chapter(&title)))
            .collect();
    }

    /// true the first time playback sits in the credits late enough in the file.
    pub fn on_position(&mut self, time: f64, duration: f64) -> bool {
        if self.completed || !(duration > 0.0) || !time.is_finite() {
            return false;
        }
        let in_ending = self.chapters.iter().enumerate().any(|(i, (start, is_ending))| {
            let end = self.chapters.get(i + 1).map(|(s, _)| *s).unwrap_or(duration);
            *is_ending && time >= *start && time < end
        });
        if in_ending && time / duration >= CREDITS_COMPLETION_RATIO {
            self.completed = true;
            return true;
        }
        false
    }

    /// true when the file ended close enough to its end to count as watched.
    pub fn on_end(&mut self, time: f64, duration: f64) -> bool {
        if self.completed || !(duration > 0.0) || !time.is_finite() {
            return false;
        }
        if time / duration >= END_FILE_COMPLETION_RATIO {
            self.completed = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(ts: f64, duration: f64) -> ProgressEntry {
        ProgressEntry {
            current_timestamp: Some(ts),
            duration: Some(duration),
            ..Default::default()
        }
    }

    fn history_item(id: u32, media_type: &str, watched_at: i64) -> WatchHistoryItem {
        WatchHistoryItem {
            id,
            media_type: media_type.to_string(),
            title: format!("item {}", id),
            poster_path: None,
            backdrop_path: None,
            release_date: None,
            vote_average: None,
            watched_at,
            current_season: None,
            current_episode: None,
            current_timestamp: None,
            number_of_seasons: None,
            last_season_number: None,
            last_season_episode_count: None,
        }
    }

    #[test]
    fn watched_needs_near_end_or_completed_flag() {
        assert!(!entry(100.0, 1000.0).is_watched());
        assert!(entry(900.0, 1000.0).is_watched());
        // past halfway with under three minutes left
        assert!(entry(1300.0, 1450.0).is_watched());
        assert!(!entry(10.0, 0.0).is_watched());
        let completed = ProgressEntry { completed: Some(true), ..Default::default() };
        assert!(completed.is_watched());
    }

    #[test]
    fn watched_flag_is_added_to_stored_values() {
        let v = with_watched_flag(&json!({ "currentTimestamp": 950, "duration": 1000 }));
        assert_eq!(v["watched"], json!(true));
        let v = with_watched_flag(&json!({ "currentTimestamp": 5, "duration": 1000 }));
        assert_eq!(v["watched"], json!(false));
    }

    #[test]
    fn finished_tv_needs_series_length_and_finale() {
        let mut item = history_item(1, "tv", 0);
        let mut progress = HashMap::new();
        progress.insert(
            "1-tv".to_string(),
            json!({ "currentTimestamp": 990, "duration": 1000, "currentSeason": 2, "currentEpisode": 10 }),
        );
        assert!(!is_media_finished(&item, &progress));

        item.last_season_number = Some(2);
        item.last_season_episode_count = Some(10);
        assert!(is_media_finished(&item, &progress));

        item.last_season_episode_count = Some(12);
        assert!(!is_media_finished(&item, &progress));
    }

    #[test]
    fn history_sorts_unfinished_before_finished() {
        let mut progress = HashMap::new();
        progress.insert("1-movie".to_string(), json!({ "currentTimestamp": 990, "duration": 1000, "updatedAt": 50, "completedAt": 50 }));
        progress.insert("2-movie".to_string(), json!({ "currentTimestamp": 10, "duration": 1000, "updatedAt": 30 }));
        progress.insert("3-movie".to_string(), json!({ "currentTimestamp": 10, "duration": 1000, "updatedAt": 40 }));
        let history = vec![history_item(1, "movie", 1), history_item(2, "movie", 1), history_item(3, "movie", 1)];

        let sorted = sort_watch_history(history, &progress);
        let ids: Vec<u32> = sorted.iter().map(|e| e.item.id).collect();
        assert_eq!(ids, vec![3, 2, 1]);
        assert!(sorted[2].finished);
        assert!(!sorted[0].finished);
    }

    #[test]
    fn resume_moves_past_watched_episodes() {
        let seasons = vec![
            SeasonInfo { season_number: 1, episode_count: 10 },
            SeasonInfo { season_number: 2, episode_count: 8 },
        ];
        let mut e = entry(500.0, 1000.0);
        e.current_season = Some(1);
        e.current_episode = Some(4);
        assert_eq!(
            resume_target(false, Some(&e), &seasons),
            Some(ResumeTarget { season: Some(1), episode: Some(4), timestamp: 500.0 })
        );

        e.completed = Some(true);
        assert_eq!(
            resume_target(false, Some(&e), &seasons),
            Some(ResumeTarget { season: Some(1), episode: Some(5), timestamp: 0.0 })
        );

        e.current_episode = Some(10);
        assert_eq!(
            resume_target(false, Some(&e), &seasons),
            Some(ResumeTarget { season: Some(2), episode: Some(1), timestamp: 0.0 })
        );

        e.current_season = Some(2);
        e.current_episode = Some(8);
        assert_eq!(resume_target(false, Some(&e), &seasons), None);
    }

    #[test]
    fn movie_resume_skips_short_and_finished_positions() {
        assert!(resume_target(true, Some(&entry(30.0, 6000.0)), &[]).is_none());
        assert!(resume_target(true, Some(&entry(5900.0, 6000.0)), &[]).is_none());
        assert_eq!(resume_target(true, Some(&entry(600.0, 6000.0)), &[]).unwrap().timestamp, 600.0);
    }

    #[test]
    fn initial_timestamp_only_resumes_matching_episode() {
        let mut e = entry(300.0, 1400.0);
        e.current_season = Some(1);
        e.current_episode = Some(2);
        assert_eq!(initial_timestamp(Some(&e), false, Some(1), Some(2)), 300.0);
        assert_eq!(initial_timestamp(Some(&e), false, Some(1), Some(3)), 0.0);
        assert_eq!(initial_timestamp(Some(&entry(300.0, 1400.0)), true, None, None), 300.0);
        assert_eq!(initial_timestamp(Some(&entry(1390.0, 1400.0)), true, None, None), 0.0);
        assert_eq!(initial_timestamp(None, true, None, None), 0.0);
    }

    #[test]
    fn chapter_titles_are_classified() {
        assert!(is_ending_chapter("Ending"));
        assert!(is_ending_chapter("End Credits"));
        assert!(!is_ending_chapter("Opening Credits"));
        assert!(is_ending_chapter("END"));
        assert!(is_skippable_chapter("Opening"));
        assert!(is_skippable_chapter("Recap"));
        assert!(!is_skippable_chapter("Part A"));
    }

    #[test]
    fn completion_tracker_fires_once_in_late_credits() {
        let mut tracker = CompletionTracker::default();
        tracker.set_chapters(vec![(0.0, "Part A".into()), (1200.0, "Ending".into()), (1300.0, "Preview".into())]);
        assert!(!tracker.on_position(600.0, 1400.0));
        assert!(tracker.on_position(1210.0, 1400.0));
        assert!(!tracker.on_position(1220.0, 1400.0));
        assert!(!tracker.on_end(1400.0, 1400.0));

        tracker.reset();
        assert!(!tracker.on_end(100.0, 1400.0));
        assert!(tracker.on_end(1200.0, 1400.0));
    }
}
