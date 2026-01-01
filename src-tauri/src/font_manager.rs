use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    pub filename: String,
    pub hash: String,
    pub path: String,
}

pub struct FontManager {
    fonts_dir: PathBuf,
}

impl FontManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self, String> {
        let app_data = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to get app data dir: {}", e))?;
        
        let fonts_dir = app_data.join("fonts");
        
        if !fonts_dir.exists() {
            fs::create_dir_all(&fonts_dir)
                .map_err(|e| format!("Failed to create fonts directory: {}", e))?;
        }
        
        Ok(Self { fonts_dir })
    }
    
    pub fn save_font(&self, filename: &str, data: &[u8]) -> Result<PathBuf, String> {
        let sanitized_name = sanitize_filename(filename);
        let font_path = self.fonts_dir.join(&sanitized_name);
        
        // Check if font already exists
        if font_path.exists() {
            println!("Font already exists: {}", sanitized_name);
            return Ok(font_path);
        }
        
        fs::write(&font_path, data)
            .map_err(|e| format!("Failed to write font file: {}", e))?;
        
        println!("saved font: {} ({} bytes)", sanitized_name, data.len());
        Ok(font_path)
    }

    pub fn get_stats(&self) -> Result<(usize, u64), String> {
        let mut count = 0;
        let mut size = 0;
        
        if let Ok(entries) = fs::read_dir(&self.fonts_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        count += 1;
                        size += metadata.len();
                    }
                }
            }
        }
        
        Ok((count, size))
    }
    
    pub fn list_fonts(&self) -> Result<Vec<FontInfo>, String> {
        let mut fonts = Vec::new();
        
        let entries = fs::read_dir(&self.fonts_dir)
            .map_err(|e| format!("Failed to read fonts directory: {}", e))?;
        
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                        let hash = format!("{:x}", md5::compute(filename));
                        fonts.push(FontInfo {
                            filename: filename.to_string(),
                            hash,
                            path: path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }
        
        Ok(fonts)
    }
    
    pub fn get_fonts_dir(&self) -> &Path {
        &self.fonts_dir
    }
}

fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(windows)]
pub fn is_font_installed(font_name: &str) -> bool {
    // Check Windows fonts directory
    let windows_fonts = PathBuf::from("C:\\Windows\\Fonts");
    
    // Extract base filename without extension for comparison
    let base_name = Path::new(font_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(font_name)
        .to_lowercase();
    
    if let Ok(entries) = fs::read_dir(&windows_fonts) {
        for entry in entries.flatten() {
            if let Some(filename) = entry.file_name().to_str() {
                let installed_base = Path::new(filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                
                if installed_base == base_name {
                    println!("Font {} already installed in system", font_name);
                    return true;
                }
            }
        }
    }
    
    false
}

#[cfg(not(windows))]
pub fn is_font_installed(font_name: &str) -> bool {
    // For non-Windows, check common font directories
    let font_dirs = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "~/.fonts",
        "~/.local/share/fonts",
    ];
    
    let base_name = Path::new(font_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(font_name)
        .to_lowercase();
    
    for dir in &font_dirs {
        let path = PathBuf::from(dir);
        if path.exists() {
            if let Ok(entries) = fs::read_dir(&path) {
                for entry in entries.flatten() {
                    if let Some(filename) = entry.file_name().to_str() {
                        let installed_base = Path::new(filename)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        
                        if installed_base == base_name {
                            println!("Font {} already installed in system", font_name);
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    false
}
