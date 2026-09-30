use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

const SUBTITLE_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub"];

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
    // the regex crate has no look-behind, so a bare E has to follow a non-alphanumeric by hand
    let re_ep = Regex::new(r"(?i)(?:Episode|Ep\.?|(?:^|[^0-9a-zA-Z])E)[\s._-]*(\d{1,3})\b").unwrap();
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

/// subtitle files imported per show, stored as `<show id>/SxxEyyy.<ext>`.
pub struct SubtitlePackStore {
    base: PathBuf,
}

impl SubtitlePackStore {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }

    fn show_dir(&self, show_id: i64) -> PathBuf {
        self.base.join(show_id.to_string())
    }

    /// imports subtitle files, folders or zip archives, filing each under the
    /// episode its name mentions.
    pub fn import(&self, show_id: i64, paths: &[String]) -> Result<ImportResult, String> {
        let dir = self.show_dir(show_id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut result = ImportResult {
            imported: 0,
            skipped: 0,
            errors: vec![],
        };
        for path_str in paths {
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

    pub fn episode_file(&self, show_id: i64, season: u32, episode: u32) -> Option<String> {
        let dir = self.show_dir(show_id);
        SUBTITLE_EXTS
            .iter()
            .map(|ext| dir.join(dest_filename(season, episode, ext)))
            .find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
    }

    /// episode numbers with a subtitle, keyed by season number.
    pub fn coverage(&self, show_id: i64) -> HashMap<String, Vec<u32>> {
        let dir = self.show_dir(show_id);
        let mut coverage: HashMap<String, Vec<u32>> = HashMap::new();
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
        coverage
    }

    pub fn remove_episode(&self, show_id: i64, season: u32, episode: u32) -> Result<(), String> {
        match self.episode_file(show_id, season, episode) {
            Some(path) => std::fs::remove_file(path).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    pub fn clear(&self, show_id: i64) -> Result<(), String> {
        let dir = self.show_dir(show_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_episode_info_from_names() {
        assert_eq!(parse_episode_info("Show.S02E05.srt"), Some((2, 5)));
        assert_eq!(parse_episode_info("Show 1x07.ass"), Some((1, 7)));
        assert_eq!(parse_episode_info("Show Episode 12.srt"), Some((1, 12)));
        assert_eq!(parse_episode_info("Show_E03.srt"), Some((1, 3)));
        assert_eq!(parse_episode_info("[Group] Show - 08 [1080p].ass"), Some((1, 8)));
    }

    #[test]
    fn imports_and_reports_coverage() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Show.S01E02.srt");
        std::fs::write(&src, "1\n00:00:01,000 --> 00:00:02,000\nhi\n").unwrap();
        let store = SubtitlePackStore::new(tmp.path().join("packs"));

        let result = store.import(7, &[src.to_string_lossy().to_string()]).unwrap();
        assert_eq!(result.imported, 1);
        assert!(store.episode_file(7, 1, 2).is_some());
        assert_eq!(store.coverage(7).get("1"), Some(&vec![2]));

        store.remove_episode(7, 1, 2).unwrap();
        assert!(store.episode_file(7, 1, 2).is_none());
        store.clear(7).unwrap();
    }
}
