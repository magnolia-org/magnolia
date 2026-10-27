//! watch tracking across progress and history.

use serde_json::Value;

use crate::watch_history::WatchHistoryItem;
use crate::watch_progress::ProgressUpdate;
use crate::watch_state::{self, ProgressEntry, ResumeTarget, SeasonInfo, WatchHistoryEntry};
use crate::Core;

impl Core {
    /// saves a player position and, when the update carries media details, adds
    /// the title to watch history. returns the saved entry, or none when the
    /// player had no usable position yet.
    pub async fn record_watch_progress(&self, mut update: ProgressUpdate) -> Option<ProgressEntry> {
        // tmdb episodes sharing one anime episode share its file, so they're watched together
        let group = match (update.season, update.episode) {
            (Some(s), Some(e)) => self.episode_group(update.media_id, &update.media_type, s, e),
            _ => Vec::new(),
        };
        if let Some(first) = group.first() {
            update.season = Some(first.0);
            update.episode = Some(first.1);
        }
        let entry = self.watch_progress.record(&update).await?;
        if group.len() > 1 {
            self.watch_progress
                .mirror_episode(update.media_id, &update.media_type, group[0], &group)
                .await;
        }

        if let Some(media) = &update.media {
            let item = WatchHistoryItem::from_media(
                update.media_id,
                &update.media_type,
                media,
                Some((entry.current_season, entry.current_episode, entry.timestamp())),
                entry.updated_at.unwrap_or_default(),
            );
            self.watch_history.add_item(item).await;
        }

        Some(entry)
    }

    /// history in display order, each flagged with whether the title is finished.
    pub async fn watch_history_sorted(&self) -> Vec<WatchHistoryEntry> {
        let history = self.watch_history.get_history().await;
        let progress = self.watch_progress.get_all().await;
        watch_state::sort_watch_history(history, &progress)
    }

    /// where to continue a title. `progress` overrides the saved position, e.g.
    /// when the player hands over "next episode" before anything was saved for it.
    pub async fn resume_target(
        &self,
        media_id: u32,
        media_type: &str,
        seasons: &[SeasonInfo],
        progress: Option<Value>,
    ) -> Option<ResumeTarget> {
        let mut entry = match progress {
            Some(value) => ProgressEntry::from_value(&value),
            None => self.watch_progress.get_entry(media_id, media_type).await,
        };
        // a split anime episode resumes at its file's first part, and moves on past its last
        if let Some(e) = entry.as_mut() {
            if let Some((s, ep)) = e.episode() {
                let group = self.episode_group(media_id, media_type, s, ep);
                let (gs, ge) = if e.is_watched() { group[group.len() - 1] } else { group[0] };
                e.current_season = Some(gs);
                e.current_episode = Some(ge);
            }
        }
        watch_state::resume_target(media_type == "movie", entry.as_ref(), seasons)
    }
}
