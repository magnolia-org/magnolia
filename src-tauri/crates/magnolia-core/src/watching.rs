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
    pub async fn record_watch_progress(&self, update: ProgressUpdate) -> Option<ProgressEntry> {
        let entry = self.watch_progress.record(&update).await?;

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
        let entry = match progress {
            Some(value) => ProgressEntry::from_value(&value),
            None => self.watch_progress.get_entry(media_id, media_type).await,
        };
        watch_state::resume_target(media_type == "movie", entry.as_ref(), seasons)
    }
}
