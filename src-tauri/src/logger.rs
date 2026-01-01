use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, Arc};
use std::thread;
use chrono::Local;
use tauri::{AppHandle, Manager};

const MAX_LOG_LENGTH: usize = 1000;

pub struct Logger {
    current_log_file: Mutex<Option<File>>,
    backend_log_file: Arc<Mutex<Option<File>>>,
    log_dir: PathBuf,
}

impl Logger {
    pub fn new(app_handle: &AppHandle) -> Result<Self, String> {
        let log_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("failed to get app data dir: {}", e))?
            .join("logs");
        
        fs::create_dir_all(&log_dir)
            .map_err(|e| format!("failed to create logs directory: {}", e))?;
        
        let logger = Logger {
            current_log_file: Mutex::new(None),
            backend_log_file: Arc::new(Mutex::new(None)),
            log_dir,
        };
        
        logger.start_new_session()?;
        logger.start_backend_session()?;
        logger.start_capturing_output();
        logger.cleanup_old_logs()?;
        
        Ok(logger)
    }
    
    fn start_new_session(&self) -> Result<(), String> {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let log_path = self.log_dir.join(format!("frontend_{}.log", timestamp));
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("failed to open log file: {}", e))?;
        
        let mut current_file = self.current_log_file.lock().unwrap();
        *current_file = Some(file);
        
        println!("started new frontend log session: {}", log_path.display());
        Ok(())
    }
    
    fn start_backend_session(&self) -> Result<(), String> {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let log_path = self.log_dir.join(format!("backend_{}.log", timestamp));
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("failed to open backend log file: {}", e))?;
        
        let mut backend_file = self.backend_log_file.lock().unwrap();
        *backend_file = Some(file);
        
        println!("started new backend log session: {}", log_path.display());
        Ok(())
    }
    
    fn start_capturing_output(&self) {
        let backend_log = Arc::clone(&self.backend_log_file);
        
        // Capture stdout in a separate thread
        let stdout_log = Arc::clone(&backend_log);
        thread::spawn(move || {
            use gag::BufferRedirect;
            use std::io::Read;
            
            let mut stdout_buffer = match BufferRedirect::stdout() {
                Ok(buf) => buf,
                Err(e) => {
                    eprintln!("failed to redirect stdout: {}", e);
                    return;
                }
            };
            
            let mut output = String::new();
            loop {
                thread::sleep(std::time::Duration::from_millis(100));
                output.clear();
                if stdout_buffer.read_to_string(&mut output).is_ok() && !output.is_empty() {
                    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let lines: Vec<&str> = output.lines().collect();
                    
                    if let Ok(mut file_guard) = stdout_log.lock() {
                        if let Some(file) = file_guard.as_mut() {
                            for line in lines {
                                let truncated = if line.len() > MAX_LOG_LENGTH {
                                    format!("{}... (truncated {} chars)", &line[..MAX_LOG_LENGTH], line.len() - MAX_LOG_LENGTH)
                                } else {
                                    line.to_string()
                                };
                                let log_line = format!("[{}] [STDOUT] {}\n", timestamp, truncated);
                                let _ = file.write_all(log_line.as_bytes());
                            }
                            let _ = file.flush();
                        }
                    }
                }
            }
        });
        
        // Capture stderr in a separate thread
        thread::spawn(move || {
            use gag::BufferRedirect;
            use std::io::Read;
            
            let mut stderr_buffer = match BufferRedirect::stderr() {
                Ok(buf) => buf,
                Err(e) => {
                    eprintln!("failed to redirect stderr: {}", e);
                    return;
                }
            };
            
            let mut output = String::new();
            loop {
                thread::sleep(std::time::Duration::from_millis(100));
                output.clear();
                if stderr_buffer.read_to_string(&mut output).is_ok() && !output.is_empty() {
                    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let lines: Vec<&str> = output.lines().collect();
                    
                    if let Ok(mut file_guard) = backend_log.lock() {
                        if let Some(file) = file_guard.as_mut() {
                            for line in lines {
                                let truncated = if line.len() > MAX_LOG_LENGTH {
                                    format!("{}... (truncated {} chars)", &line[..MAX_LOG_LENGTH], line.len() - MAX_LOG_LENGTH)
                                } else {
                                    line.to_string()
                                };
                                let log_line = format!("[{}] [STDERR] {}\n", timestamp, truncated);
                                let _ = file.write_all(log_line.as_bytes());
                            }
                            let _ = file.flush();
                        }
                    }
                }
            }
        });
    }
    
    fn cleanup_old_logs(&self) -> Result<(), String> {
        // Cleanup frontend logs
        let mut frontend_logs: Vec<_> = fs::read_dir(&self.log_dir)
            .map_err(|e| format!("failed to read logs directory: {}", e))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().is_file() 
                    && entry.file_name().to_string_lossy().starts_with("frontend_")
                    && entry.file_name().to_string_lossy().ends_with(".log")
            })
            .collect();
        
        frontend_logs.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        
        // Keep only the 3 most recent frontend sessions
        while frontend_logs.len() > 3 {
            if let Some(entry) = frontend_logs.first() {
                if let Err(e) = fs::remove_file(entry.path()) {
                    eprintln!("failed to remove old frontend log file: {}", e);
                }
            }
            frontend_logs.remove(0);
        }
        
        // Cleanup backend logs
        let mut backend_logs: Vec<_> = fs::read_dir(&self.log_dir)
            .map_err(|e| format!("failed to read logs directory: {}", e))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().is_file() 
                    && entry.file_name().to_string_lossy().starts_with("backend_")
                    && entry.file_name().to_string_lossy().ends_with(".log")
            })
            .collect();
        
        backend_logs.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        
        // Keep only the 3 most recent backend sessions
        while backend_logs.len() > 3 {
            if let Some(entry) = backend_logs.first() {
                if let Err(e) = fs::remove_file(entry.path()) {
                    eprintln!("failed to remove old backend log file: {}", e);
                }
            }
            backend_logs.remove(0);
        }
        
        Ok(())
    }
    
    pub fn log(&self, level: &str, message: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let truncated_msg = if message.len() > MAX_LOG_LENGTH {
            format!("{}... (truncated {} chars)", &message[..MAX_LOG_LENGTH], message.len() - MAX_LOG_LENGTH)
        } else {
            message.to_string()
        };
        let log_line = format!("[{}] [{}] {}\n", timestamp, level, truncated_msg);
        
        if let Ok(mut file_guard) = self.current_log_file.lock() {
            if let Some(file) = file_guard.as_mut() {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }
    
    #[allow(dead_code)]
    pub fn log_backend(&self, level: &str, message: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let truncated_msg = if message.len() > MAX_LOG_LENGTH {
            format!("{}... (truncated {} chars)", &message[..MAX_LOG_LENGTH], message.len() - MAX_LOG_LENGTH)
        } else {
            message.to_string()
        };
        let log_line = format!("[{}] [{}] {}\n", timestamp, level, truncated_msg);
        
        if let Ok(mut file_guard) = self.backend_log_file.lock() {
            if let Some(file) = file_guard.as_mut() {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }
}

#[tauri::command]
pub fn log_message(level: String, message: String, logger: tauri::State<Logger>) {
    logger.log(&level, &message);
}

// Macro for easy backend logging
#[macro_export]
macro_rules! log_backend {
    ($logger:expr, $level:expr, $($arg:tt)*) => {
        $logger.log_backend($level, &format!($($arg)*))
    };
}
