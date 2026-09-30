//! tauri commands, thin wrappers over magnolia-core.

pub mod extensions;
pub mod library;
pub mod search;
pub mod subtitles;
pub mod torrents;

use magnolia_core::Core;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub type CoreState<'a> = tauri::State<'a, Arc<Core>>;

/// tells the frontend stores to reload watch progress and history.
pub fn notify_watch_state_changed(app: &AppHandle) {
    if let Err(e) = app.emit("watch-state-changed", ()) {
        log::warn!("failed to emit watch-state-changed: {}", e);
    }
}
