//! deciding what to play, from which torrent file, and where to start.

use serde::{Deserialize, Serialize};

use crate::extensions::runtime::DebridListContext;
use crate::extensions::LoadedExtension;
use crate::files::{self, MediaFile};
use crate::watch_state;
use crate::Core;

/// the video files of a torrent. `handle_id` is the local torrent handle, or
/// none when a debrid service will stream it instead.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentFiles {
    pub handle_id: Option<usize>,
    pub files: Vec<MediaFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackRequest {
    pub media_id: u32,
    pub media_type: String,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    pub magnet_link: String,
    pub file_index: usize,
    /// a handle from an earlier `torrent_files` call, reused instead of adding the torrent again
    #[serde(default)]
    pub handle_id: Option<usize>,
}

/// everything the player needs to start.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackPlan {
    pub handle_id: Option<usize>,
    pub file_index: usize,
    pub magnet_link: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub initial_timestamp: f64,
}

fn tracking_slot(media_type: &str, season: Option<u32>, episode: Option<u32>) -> (u32, u32) {
    // movies are tracked as season 0, episode 0
    if media_type == "movie" {
        (0, 0)
    } else {
        (season.unwrap_or(0), episode.unwrap_or(0))
    }
}

impl Core {
    /// the debrid extension chosen as streaming client, or none for the built-in torrent pipeline.
    pub async fn streaming_client(&self) -> Option<LoadedExtension> {
        let client_id = self.settings.get().await.streaming_client;
        if client_id == "builtin" {
            return None;
        }
        match self.extensions.get(&client_id).await {
            Some(ext) if ext.enabled && ext.manifest.ext_type == "debrid" => Some(ext),
            _ => {
                log::warn!("streaming client '{}' unavailable, falling back to built-in", client_id);
                None
            }
        }
    }

    /// lists the video files of a magnet, through the debrid client when one is
    /// active, otherwise by resolving its metadata with librqbit.
    pub async fn torrent_files(
        &self,
        magnet_link: &str,
        season: Option<u32>,
        episode: Option<u32>,
        media_type: Option<String>,
    ) -> Result<TorrentFiles, String> {
        if let Some(debrid) = self.streaming_client().await {
            let ctx = DebridListContext {
                magnet: magnet_link.to_string(),
                season,
                episode,
                media_type,
            };
            let listed = self.extensions.list_debrid_files(&debrid.id, ctx).await?;
            let files = listed
                .into_iter()
                .filter(|f| files::is_video_file(&f.name))
                .map(|f| MediaFile {
                    index: f.index.max(0) as usize,
                    name: f.name,
                    size: f.size,
                    path: f.path,
                })
                .collect();
            return Ok(TorrentFiles { handle_id: None, files });
        }

        let handle_id = self
            .torrents
            .add_torrent(magnet_link.to_string())
            .await
            .map_err(|e| e.to_string())?;
        let info = self.torrents.get_torrent_info(handle_id).await.map_err(|e| e.to_string())?;
        let files = info
            .files
            .into_iter()
            .map(|f| MediaFile {
                index: f.index,
                name: f.name,
                size: f.size,
                path: f.path,
            })
            .collect();
        Ok(TorrentFiles {
            handle_id: Some(handle_id),
            files,
        })
    }

    /// finds the requested episode in a torrent and remembers it. for season
    /// packs every other numbered episode is remembered too. none means the
    /// user has to pick the file by hand.
    pub async fn auto_assign_files(
        &self,
        show_id: u32,
        media_type: &str,
        season: u32,
        episode: u32,
        magnet_link: &str,
        files: &[MediaFile],
    ) -> Option<MediaFile> {
        let videos: Vec<MediaFile> = files.iter().filter(|f| files::is_video_file(&f.name)).cloned().collect();
        let matched = videos.get(files::find_episode_file(&videos, season, episode)?)?.clone();

        self.selections
            .save_selection(show_id, season, episode, magnet_link.to_string(), matched.index)
            .await;

        if media_type != "movie" && videos.len() > 1 {
            let batch: Vec<(u32, u32, String, usize)> = videos
                .iter()
                .filter(|f| f.index != matched.index)
                .filter_map(|f| {
                    let (s, e) = files::parse_episode_number(&f.name, season)?;
                    Some((s, e, magnet_link.to_string(), f.index))
                })
                .collect();
            if !batch.is_empty() {
                log::info!("auto-assigned {} more episodes from {}", batch.len(), magnet_link);
                self.selections.save_multiple_selections(show_id, batch).await;
            }
        }

        Some(matched)
    }

    /// readies a chosen torrent file for the player. if the torrent can't be
    /// added its saved selection is dropped, so the next play asks for a new source.
    pub async fn prepare_playback(&self, req: PlaybackRequest) -> Result<PlaybackPlan, String> {
        let is_movie = req.media_type == "movie";
        let (season, episode) = if is_movie { (None, None) } else { (req.season, req.episode) };

        let handle_id = match req.handle_id {
            Some(id) => Some(id),
            // a debrid client streams remotely from the magnet, so it needs no local handle
            None if self.streaming_client().await.is_some() => None,
            None => match self.torrents.add_torrent(req.magnet_link.clone()).await {
                Ok(id) => Some(id),
                Err(e) => {
                    if let (Some(s), Some(ep)) = (season, episode) {
                        self.selections.remove_selection(req.media_id, s, ep).await;
                    }
                    return Err(e.to_string());
                }
            },
        };

        let progress = self.watch_progress.get_entry(req.media_id, &req.media_type).await;
        Ok(PlaybackPlan {
            handle_id,
            file_index: req.file_index,
            magnet_link: req.magnet_link,
            season,
            episode,
            initial_timestamp: watch_state::initial_timestamp(progress.as_ref(), is_movie, season, episode),
        })
    }

    /// plays an episode (or movie) straight from its remembered torrent file.
    /// none when nothing was picked for it yet.
    pub async fn prepare_saved_playback(
        &self,
        media_id: u32,
        media_type: &str,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Option<PlaybackPlan>, String> {
        let (slot_season, slot_episode) = tracking_slot(media_type, season, episode);
        let Some(saved) = self.selections.get_selection(media_id, slot_season, slot_episode).await else {
            return Ok(None);
        };
        self.prepare_playback(PlaybackRequest {
            media_id,
            media_type: media_type.to_string(),
            season,
            episode,
            magnet_link: saved.magnet_link,
            file_index: saved.file_index,
            handle_id: None,
        })
        .await
        .map(Some)
    }

    /// continues a title from its saved progress without opening its details.
    /// none when the details view has to decide, e.g. a finished episode means
    /// the next one is up and only the details know the season layout.
    pub async fn plan_quick_play(&self, media_id: u32, media_type: &str) -> Result<Option<PlaybackPlan>, String> {
        if media_type == "movie" {
            return self.prepare_saved_playback(media_id, media_type, None, None).await;
        }
        let progress = self.watch_progress.get_entry(media_id, media_type).await;
        if progress.as_ref().is_some_and(|p| p.is_watched()) {
            return Ok(None);
        }
        let (season, episode) = progress.and_then(|p| p.episode()).unwrap_or((1, 1));
        self.prepare_saved_playback(media_id, media_type, Some(season), Some(episode)).await
    }
}
