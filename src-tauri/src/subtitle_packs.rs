use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

const SUBTITLE_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub"];

fn show_dir(app_handle: &AppHandle, show_id: i64) -> Result<PathBuf, String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let dir = base.join("subtitle_packs").join(show_id.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn is_subtitle(ext: &str) -> bool {
    SUBTITLE_EXTS.contains(&ext.to_lowercase().as_str())
}

/// Parse season+episode from a filename stem.
/// Returns (season, episode). Season defaults to 1 if not found.
fn parse_episode_info(filename: &str) -> Option<(u32, u32)> {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    // S01E02 / s1e2
    let re_sxe = Regex::new(r"(?i)S(\d{1,2})E(\d{1,3})").unwrap();
    if let Some(cap) = re_sxe.captures(stem) {
        let s: u32 = cap[1].parse().ok()?;
        let e: u32 = cap[2].parse().ok()?;
        return Some((s, e));
    }

    // 1x02
    let re_1x = Regex::new(r"(?i)\b(\d{1,2})x(\d{2,3})\b").unwrap();
    if let Some(cap) = re_1x.captures(stem) {
        let s: u32 = cap[1].parse().ok()?;
        let e: u32 = cap[2].parse().ok()?;
        return Some((s, e));
    }

    // E02 / EP02 / Episode 02
    let re_ep = Regex::new(r"(?i)(?:Episode|Ep\.?|(?<![0-9a-zA-Z])E)[\s._-]*(\d{1,3})\b").unwrap();
    if let Some(cap) = re_ep.captures(stem) {
        let e: u32 = cap[1].parse().ok()?;
        if e > 0 {
            return Some((1, e));
        }
    }

    // Standalone 2-3 digit number between non-digit delimiters (e.g. " - 05 -", "[12]")
    let re_num = Regex::new(r"(?:^|[-\[\s._])(\d{2,3})(?:[-\]\s._]|$)").unwrap();
    for cap in re_num.captures_iter(stem) {
        let e: u32 = cap[1].parse().ok()?;
        if e > 0 && e < 500 {
            return Some((1, e));
        }
    }

    None
}

fn dest_filename(season: u32, episode: u32, ext: &str) -> String {
    format!("S{:02}E{:03}.{}", season, episode, ext.to_lowercase())
}

fn process_single_file(path: &Path, dir: &Path, result: &mut ImportResult) {
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return,
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !is_subtitle(&ext) {
        return;
    }
    match parse_episode_info(filename) {
        Some((season, episode)) => {
            let dest = dir.join(dest_filename(season, episode, &ext));
            match std::fs::copy(path, &dest) {
                Ok(_) => result.imported += 1,
                Err(e) => result.errors.push(format!("{}: {}", filename, e)),
            }
        }
        None => {
            result.skipped += 1;
        }
    }
}

fn process_dir(src_dir: &Path, show_dir: &Path, result: &mut ImportResult) {
    let Ok(entries) = std::fs::read_dir(src_dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            process_dir(&p, show_dir, result);
        } else {
            process_single_file(&p, show_dir, result);
        }
    }
}

fn process_zip(zip_path: &Path, show_dir: &Path, result: &mut ImportResult) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut zf = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = zf.name().to_string();
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !is_subtitle(&ext) {
            continue;
        }
        let basename = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&name)
            .to_string();
        match parse_episode_info(&basename) {
            Some((season, episode)) => {
                let dest = show_dir.join(dest_filename(season, episode, &ext));
                let mut content = Vec::new();
                if let Err(e) = zf.read_to_end(&mut content) {
                    result.errors.push(format!("{}: {}", basename, e));
                    continue;
                }
                match std::fs::write(&dest, &content) {
                    Ok(_) => result.imported += 1,
                    Err(e) => result.errors.push(format!("{}: {}", basename, e)),
                }
            }
            None => {
                result.skipped += 1;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn import_subtitle_pack(
    app_handle: AppHandle,
    show_id: i64,
    paths: Vec<String>,
) -> Result<ImportResult, String> {
    let dir = show_dir(&app_handle, show_id)?;
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: vec![],
    };
    for path_str in &paths {
        let path = Path::new(path_str);
        if !path.exists() {
            result.errors.push(format!("{}: not found", path_str));
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if path.is_dir() {
            process_dir(path, &dir, &mut result);
        } else if ext == "zip" {
            if let Err(e) = process_zip(path, &dir, &mut result) {
                result.errors.push(format!("{}: {}", path_str, e));
            }
        } else {
            process_single_file(path, &dir, &mut result);
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_subtitle_pack_for_episode(
    app_handle: AppHandle,
    show_id: i64,
    season: u32,
    episode: u32,
) -> Result<Option<String>, String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let dir = base.join("subtitle_packs").join(show_id.to_string());
    if !dir.exists() {
        return Ok(None);
    }
    for ext in SUBTITLE_EXTS {
        let p = dir.join(dest_filename(season, episode, ext));
        if p.exists() {
            return Ok(Some(p.to_string_lossy().to_string()));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn get_subtitle_pack_coverage(
    app_handle: AppHandle,
    show_id: i64,
) -> Result<HashMap<String, Vec<u32>>, String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let dir = base.join("subtitle_packs").join(show_id.to_string());
    let mut coverage: HashMap<String, Vec<u32>> = HashMap::new();
    if !dir.exists() {
        return Ok(coverage);
    }
    let re = Regex::new(r"(?i)^S(\d{1,2})E(\d{1,3})\.(?:srt|ass|ssa|vtt|sub)$").unwrap();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if let Some(cap) = re.captures(&name_str) {
                let s: u32 = cap[1].parse().unwrap_or(0);
                let e: u32 = cap[2].parse().unwrap_or(0);
                if s > 0 && e > 0 {
                    coverage.entry(s.to_string()).or_default().push(e);
                }
            }
        }
    }
    for eps in coverage.values_mut() {
        eps.sort_unstable();
    }
    Ok(coverage)
}

#[tauri::command]
pub async fn remove_subtitle_pack_episode(
    app_handle: AppHandle,
    show_id: i64,
    season: u32,
    episode: u32,
) -> Result<(), String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let dir = base.join("subtitle_packs").join(show_id.to_string());
    for ext in SUBTITLE_EXTS {
        let p = dir.join(dest_filename(season, episode, ext));
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn clear_subtitle_pack(
    app_handle: AppHandle,
    show_id: i64,
) -> Result<(), String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let dir = base.join("subtitle_packs").join(show_id.to_string());
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}
