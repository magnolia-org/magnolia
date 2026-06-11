//! Rhai extension system.
//!
//! Extensions are single `.rhai` scripts that declare a `manifest()` function
//! and one entry point: `search(ctx)` for trackers or `fetch_subtitles(ctx)`
//! for subtitle sources. Scripts live in `<app data dir>/extensions/`, with
//! install state and per-extension field values in `extensions.json` next to
//! them. The four original built-in trackers ship as preinstalled extensions.

pub mod runtime;

use crate::subtitles::Subtitle;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

const PREINSTALLED: &[(&str, &str)] = &[
    ("nyaa", include_str!("../../extensions/nyaa.rhai")),
    ("limetorrents", include_str!("../../extensions/limetorrents.rhai")),
    ("thepiratebay", include_str!("../../extensions/thepiratebay.rhai")),
    ("eztv", include_str!("../../extensions/eztv.rhai")),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(rename = "type", default = "default_field_type")]
    pub field_type: String, // text | number | checkbox | select
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

fn default_field_type() -> String {
    "text".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// 64x64 icon, base64-encoded PNG.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(rename = "type")]
    pub ext_type: String, // "tracker" | "subtitles"
    #[serde(default)]
    pub tracker_name: Option<String>,
    #[serde(default)]
    pub is_anime: Option<bool>,
    #[serde(default)]
    pub subtitle_source_name: Option<String>,
    #[serde(default)]
    pub can_auto_fetch: Option<bool>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
}

impl ExtensionManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("manifest: 'name' is required".to_string());
        }
        match self.ext_type.as_str() {
            "tracker" => {
                if self.tracker_name.as_deref().map_or(true, |s| s.trim().is_empty()) {
                    return Err("manifest: 'tracker_name' is required for tracker extensions".to_string());
                }
                if self.is_anime.is_none() {
                    return Err("manifest: 'is_anime' is required for tracker extensions".to_string());
                }
            }
            "subtitles" => {
                if self
                    .subtitle_source_name
                    .as_deref()
                    .map_or(true, |s| s.trim().is_empty())
                {
                    return Err(
                        "manifest: 'subtitle_source_name' is required for subtitle extensions"
                            .to_string(),
                    );
                }
                if self.can_auto_fetch.is_none() {
                    return Err(
                        "manifest: 'can_auto_fetch' is required for subtitle extensions".to_string(),
                    );
                }
            }
            other => {
                return Err(format!(
                    "manifest: 'type' must be 'tracker' or 'subtitles', got '{}'",
                    other
                ))
            }
        }
        let mut seen = std::collections::HashSet::new();
        for field in &self.fields {
            if field.key.trim().is_empty() {
                return Err("manifest: field 'key' must not be empty".to_string());
            }
            if !seen.insert(field.key.clone()) {
                return Err(format!("manifest: duplicate field key '{}'", field.key));
            }
            match field.field_type.as_str() {
                "text" | "number" | "checkbox" => {}
                "select" => {
                    if field.options.as_ref().map_or(true, |o| o.is_empty()) {
                        return Err(format!(
                            "manifest: select field '{}' needs non-empty 'options'",
                            field.key
                        ));
                    }
                }
                other => {
                    return Err(format!(
                        "manifest: field '{}' has unknown type '{}'",
                        field.key, other
                    ))
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtensionRecord {
    id: String,
    origin: String, // "preinstalled" | "file:<path>" | "url:<url>"
    enabled: bool,
    #[serde(default)]
    field_values: HashMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    seeded: bool,
    #[serde(default)]
    extensions: Vec<ExtensionRecord>,
}

#[derive(Clone)]
pub struct LoadedExtension {
    pub id: String,
    pub origin: String,
    pub enabled: bool,
    pub manifest: ExtensionManifest,
    pub field_values: HashMap<String, String>,
    pub source: String,
}

#[derive(Serialize)]
pub struct ExtensionInfo {
    pub id: String,
    pub origin: String,
    pub enabled: bool,
    pub manifest: ExtensionManifest,
    pub field_values: HashMap<String, String>,
}

impl From<&LoadedExtension> for ExtensionInfo {
    fn from(ext: &LoadedExtension) -> Self {
        Self {
            id: ext.id.clone(),
            origin: ext.origin.clone(),
            enabled: ext.enabled,
            manifest: ext.manifest.clone(),
            field_values: ext.field_values.clone(),
        }
    }
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = true;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "extension".to_string()
    } else {
        slug
    }
}

pub struct ExtensionManager {
    dir: PathBuf,
    state_path: PathBuf,
    extensions: RwLock<Vec<LoadedExtension>>,
}

impl ExtensionManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let dir = app_data_dir.join("extensions");
        let _ = fs::create_dir_all(&dir);
        let state_path = dir.join("extensions.json");

        let mut state: StateFile = if state_path.exists() {
            fs::read_to_string(&state_path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            StateFile::default()
        };

        // Preinstalled extensions are always present (they can be disabled
        // but not uninstalled). Keep their script files in sync with the
        // bundled versions so app updates ship fixes, preserving the user's
        // enabled state and field values.
        for (id, source) in PREINSTALLED {
            let path = dir.join(format!("{}.rhai", id));
            let needs_write = fs::read_to_string(&path)
                .map(|current| current != *source)
                .unwrap_or(true);
            if needs_write {
                if let Err(e) = fs::write(&path, source) {
                    log::error!("failed to seed extension {}: {}", id, e);
                    continue;
                }
            }
            if let Some(record) = state.extensions.iter_mut().find(|r| r.id == *id) {
                record.origin = "preinstalled".to_string();
            } else {
                state.extensions.push(ExtensionRecord {
                    id: id.to_string(),
                    origin: "preinstalled".to_string(),
                    enabled: true,
                    field_values: HashMap::new(),
                });
            }
        }
        state.seeded = true;

        let mut loaded = Vec::new();
        for record in &state.extensions {
            let path = dir.join(format!("{}.rhai", record.id));
            let source = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("extension {}: failed to read script: {}", record.id, e);
                    continue;
                }
            };
            match runtime::parse_manifest(&source) {
                Ok(manifest) => loaded.push(LoadedExtension {
                    id: record.id.clone(),
                    origin: record.origin.clone(),
                    enabled: record.enabled,
                    manifest,
                    field_values: record.field_values.clone(),
                    source,
                }),
                Err(e) => log::error!("extension {}: {}", record.id, e),
            }
        }

        let manager = Self {
            dir,
            state_path,
            extensions: RwLock::new(loaded),
        };
        manager.save_state_from(&state.extensions);
        manager
    }

    fn save_state_from(&self, records: &[ExtensionRecord]) {
        let state = StateFile {
            seeded: true,
            extensions: records.to_vec(),
        };
        match serde_json::to_string_pretty(&state) {
            Ok(content) => {
                if let Err(e) = fs::write(&self.state_path, content) {
                    log::error!("failed to write extensions state: {}", e);
                }
            }
            Err(e) => log::error!("failed to serialize extensions state: {}", e),
        }
    }

    async fn save_state(&self) {
        let exts = self.extensions.read().await;
        let records: Vec<ExtensionRecord> = exts
            .iter()
            .map(|e| ExtensionRecord {
                id: e.id.clone(),
                origin: e.origin.clone(),
                enabled: e.enabled,
                field_values: e.field_values.clone(),
            })
            .collect();
        self.save_state_from(&records);
    }

    pub async fn list(&self) -> Vec<ExtensionInfo> {
        self.extensions.read().await.iter().map(Into::into).collect()
    }

    pub async fn get(&self, id: &str) -> Option<LoadedExtension> {
        self.extensions
            .read()
            .await
            .iter()
            .find(|e| e.id == id)
            .cloned()
    }

    pub async fn enabled_trackers(&self) -> Vec<LoadedExtension> {
        self.extensions
            .read()
            .await
            .iter()
            .filter(|e| e.enabled && e.manifest.ext_type == "tracker")
            .cloned()
            .collect()
    }

    pub async fn enabled_subtitle_extensions(&self) -> Vec<LoadedExtension> {
        self.extensions
            .read()
            .await
            .iter()
            .filter(|e| e.enabled && e.manifest.ext_type == "subtitles")
            .cloned()
            .collect()
    }

    /// Install (or update) an extension from script source. The manifest must
    /// already be parsed and validated by the caller (off the async runtime).
    pub async fn install(
        &self,
        source: String,
        manifest: ExtensionManifest,
        origin: String,
    ) -> Result<ExtensionInfo, String> {
        let id = slugify(&manifest.name);
        if PREINSTALLED.iter().any(|(pid, _)| *pid == id) {
            return Err(format!(
                "'{}' collides with a preinstalled extension and can't be replaced",
                manifest.name
            ));
        }
        let path = self.dir.join(format!("{}.rhai", id));
        fs::write(&path, &source).map_err(|e| format!("failed to write extension: {}", e))?;

        {
            let mut exts = self.extensions.write().await;
            if let Some(existing) = exts.iter_mut().find(|e| e.id == id) {
                existing.manifest = manifest;
                existing.source = source;
                existing.origin = origin;
                existing.enabled = true;
            } else {
                exts.push(LoadedExtension {
                    id: id.clone(),
                    origin,
                    enabled: true,
                    manifest,
                    field_values: HashMap::new(),
                    source,
                });
            }
        }
        self.save_state().await;
        let ext = self.get(&id).await.ok_or("extension vanished after install")?;
        Ok((&ext).into())
    }

    pub async fn remove(&self, id: &str) -> Result<(), String> {
        {
            let mut exts = self.extensions.write().await;
            let ext = exts
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(|| format!("extension '{}' not found", id))?;
            if ext.origin == "preinstalled" {
                return Err(
                    "Preinstalled extensions can't be removed — disable them instead.".to_string(),
                );
            }
            exts.retain(|e| e.id != id);
        }
        let _ = fs::remove_file(self.dir.join(format!("{}.rhai", id)));
        self.save_state().await;
        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        {
            let mut exts = self.extensions.write().await;
            let ext = exts
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| format!("extension '{}' not found", id))?;
            ext.enabled = enabled;
        }
        self.save_state().await;
        Ok(())
    }

    pub async fn set_field_values(
        &self,
        id: &str,
        values: HashMap<String, String>,
    ) -> Result<(), String> {
        {
            let mut exts = self.extensions.write().await;
            let ext = exts
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| format!("extension '{}' not found", id))?;
            ext.field_values = values;
        }
        self.save_state().await;
        Ok(())
    }
}

async fn parse_manifest_blocking(source: String) -> Result<(String, ExtensionManifest), String> {
    // Manifest parsing executes script top-level code, which may (badly) call
    // blocking host functions — keep it off the async runtime.
    tokio::task::spawn_blocking(move || {
        let manifest = runtime::parse_manifest(&source)?;
        Ok((source, manifest))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preinstalled_manifests_parse_and_ids_match() {
        for (id, source) in PREINSTALLED {
            let manifest = runtime::parse_manifest(source)
                .unwrap_or_else(|e| panic!("{}: {}", id, e));
            assert_eq!(manifest.ext_type, "tracker", "{}", id);
            assert_eq!(&slugify(&manifest.name), id);
        }
    }

    fn run_search(id: &str, query: &str, imdb_id: Option<&str>) -> Vec<crate::search::SearchResult> {
        let (_, source) = PREINSTALLED.iter().find(|(i, _)| *i == id).unwrap();
        let manifest = runtime::parse_manifest(source).unwrap();
        let ctx = runtime::TrackerSearchContext {
            query: query.to_string(),
            media_type: None,
            imdb_id: imdb_id.map(String::from),
            season: None,
            episode: None,
        };
        runtime::run_tracker_search(source, &manifest, &HashMap::new(), &ctx)
            .unwrap_or_else(|e| panic!("{}: {}", id, e))
    }

    // Live network tests — run with: cargo test -- --ignored
    #[test]
    #[ignore]
    fn live_nyaa_search() {
        let results = run_search("nyaa", "one piece", None);
        assert!(!results.is_empty());
        assert!(results[0].magnet_link.starts_with("magnet:"));
    }

    #[test]
    #[ignore]
    fn live_piratebay_search() {
        let results = run_search("thepiratebay", "breaking bad", Some("tt0903747"));
        assert!(!results.is_empty());
        assert!(results[0].magnet_link.starts_with("magnet:"));
    }

    #[test]
    #[ignore]
    fn live_eztv_search() {
        let results = run_search("eztv", "breaking bad", Some("tt0903747"));
        assert!(!results.is_empty());
    }

    // Dev helper for extension authors: validates an arbitrary script.
    // MAGNOLIA_EXT_SCRIPT=/path/to/ext.rhai cargo test validate_external_script -- --ignored --nocapture
    #[test]
    #[ignore]
    fn validate_external_script() {
        let Some(path) = std::env::var_os("MAGNOLIA_EXT_SCRIPT") else {
            println!("MAGNOLIA_EXT_SCRIPT not set, skipping");
            return;
        };
        let source = std::fs::read_to_string(&path).expect("failed to read script");
        let manifest = runtime::parse_manifest(&source).expect("manifest error");
        println!("manifest OK: {} v{} ({})", manifest.name, manifest.version.as_deref().unwrap_or("?"), manifest.ext_type);

        if manifest.ext_type == "tracker" {
            let ctx = runtime::TrackerSearchContext {
                query: "test query".to_string(),
                media_type: Some("tv".to_string()),
                imdb_id: Some("tt0903747".to_string()),
                season: Some(1),
                episode: Some(2),
            };
            let results = runtime::run_tracker_search(&source, &manifest, &HashMap::new(), &ctx)
                .expect("search error");
            println!("search OK: {} results", results.len());
            for r in results.iter().take(3) {
                println!("  {} | {} | s{} p{}", r.title, r.size, r.seeds, r.peers);
            }
        } else {
            let ctx = runtime::SubtitleFetchContext {
                tmdb_id: "1396".to_string(),
                media_type: "tv".to_string(),
                season: Some(1),
                episode: Some(2),
            };
            let results =
                runtime::run_subtitle_fetch("test", &source, &manifest, &HashMap::new(), &ctx)
                    .expect("fetch_subtitles error");
            println!("fetch_subtitles OK: {} results", results.len());
        }
    }

    #[test]
    #[ignore]
    fn live_limetorrents_search() {
        let results = run_search("limetorrents", "breaking bad", None);
        // LimeTorrents domains rotate; don't hard-fail on emptiness, just on errors.
        for r in &results {
            assert!(r.magnet_link.starts_with("magnet:"));
        }
    }
}

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_extensions(
    manager: State<'_, Arc<ExtensionManager>>,
) -> Result<Vec<ExtensionInfo>, String> {
    Ok(manager.list().await)
}

#[tauri::command]
pub async fn install_extension_from_path(
    manager: State<'_, Arc<ExtensionManager>>,
    path: String,
) -> Result<ExtensionInfo, String> {
    let source =
        fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {}", path, e))?;
    let (source, manifest) = parse_manifest_blocking(source).await?;
    manager.install(source, manifest, format!("file:{}", path)).await
}

#[tauri::command]
pub async fn install_extension_from_url(
    manager: State<'_, Arc<ExtensionManager>>,
    url: String,
) -> Result<ExtensionInfo, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("download failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("download failed: HTTP {}", response.status()));
    }
    let source = response
        .text()
        .await
        .map_err(|e| format!("failed to read response: {}", e))?;
    if source.len() > 1_048_576 {
        return Err("extension script too large (max 1 MB)".to_string());
    }
    let (source, manifest) = parse_manifest_blocking(source).await?;
    manager.install(source, manifest, format!("url:{}", url)).await
}

#[tauri::command]
pub async fn remove_extension(
    manager: State<'_, Arc<ExtensionManager>>,
    id: String,
) -> Result<(), String> {
    manager.remove(&id).await
}

#[tauri::command]
pub async fn set_extension_enabled(
    manager: State<'_, Arc<ExtensionManager>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    manager.set_enabled(&id, enabled).await
}

#[tauri::command]
pub async fn set_extension_field_values(
    manager: State<'_, Arc<ExtensionManager>>,
    id: String,
    values: HashMap<String, String>,
) -> Result<(), String> {
    manager.set_field_values(&id, values).await
}

/// Run subtitle extensions for the given media. With `auto_only`, only
/// extensions whose manifest declares `can_auto_fetch: true` run (used when
/// playback starts); otherwise only manual ones run (the "Fetch Subtitles"
/// button in the player).
#[tauri::command]
pub async fn fetch_extension_subtitles(
    manager: State<'_, Arc<ExtensionManager>>,
    tmdb_id: String,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
    auto_only: bool,
) -> Result<Vec<Subtitle>, String> {
    let extensions: Vec<LoadedExtension> = manager
        .enabled_subtitle_extensions()
        .await
        .into_iter()
        .filter(|e| e.manifest.can_auto_fetch.unwrap_or(false) == auto_only)
        .collect();

    let mut handles = Vec::new();
    for ext in extensions {
        let ctx = runtime::SubtitleFetchContext {
            tmdb_id: tmdb_id.clone(),
            media_type: media_type.clone(),
            season,
            episode,
        };
        handles.push(tokio::task::spawn_blocking(move || {
            runtime::run_subtitle_fetch(&ext.id, &ext.source, &ext.manifest, &ext.field_values, &ctx)
                .map_err(|e| (ext.id.clone(), e))
        }));
    }

    let mut all = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(subs)) => all.extend(subs),
            Ok(Err((id, e))) => log::warn!("subtitle extension {} failed: {}", id, e),
            Err(e) => log::warn!("subtitle extension task panicked: {}", e),
        }
    }
    Ok(all)
}
