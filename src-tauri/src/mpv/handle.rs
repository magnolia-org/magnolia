use super::event_loop::mpv_event_loop;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::ffi::ensure_numeric_locale_for_mpv;
use super::ffi::{
    mpv_command, mpv_create, mpv_create_client, mpv_destroy, mpv_format, mpv_free,
    mpv_get_property_string, mpv_initialize, mpv_set_option, mpv_set_option_string,
    mpv_terminate_destroy, SoiaUtils, soia_utils_create, soia_utils_destroy,
    soia_utils_render_context_update, soia_utils_render_target_resize,
    soia_utils_uses_render_context,
};
use log::info;
use std::ffi::{c_void, CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tauri::AppHandle;

pub struct MpvHandle {
    ctx: AtomicPtr<c_void>,
    is_playing: Arc<AtomicBool>,
    eof_reached: Arc<AtomicBool>,
    app_handle: AppHandle,
    event_loop_stop: Arc<AtomicBool>,
    event_loop_handle: Mutex<Option<JoinHandle<()>>>,
    render_loop_stop: Arc<AtomicBool>,
    render_loop_handle: Mutex<Option<JoinHandle<()>>>,
    /// soia_utils instance (all platforms).
    soia_utils: AtomicPtr<SoiaUtils>,
}

unsafe impl Send for MpvHandle {}
unsafe impl Sync for MpvHandle {}

/// Write the app's bundled Geist fonts to `<app data>/fonts` so libass can
/// load them via `sub-fonts-dir` — subtitles then default to the same
/// typeface as the UI without requiring the font to be installed system-wide.
fn seed_subtitle_fonts(app_handle: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    const FONTS: &[(&str, &[u8])] = &[
        (
            "Geist-Regular.ttf",
            include_bytes!("../../resources/fonts/Geist-Regular.ttf"),
        ),
        (
            "Geist-Bold.ttf",
            include_bytes!("../../resources/fonts/Geist-Bold.ttf"),
        ),
    ];
    let dir = app_handle.path().app_data_dir().ok()?.join("fonts");
    std::fs::create_dir_all(&dir).ok()?;
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        let needs_write = std::fs::metadata(&path)
            .map(|m| m.len() != bytes.len() as u64)
            .unwrap_or(true);
        if needs_write {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("failed to seed subtitle font {}: {}", name, e);
            }
        }
    }
    Some(dir)
}

fn release_mpv_context(ctx: &AtomicPtr<c_void>, terminate: bool) {
    let ptr = ctx.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if ptr.is_null() {
        return;
    }
    info!("Freeing MPV player handle...");
    unsafe {
        if terminate {
            mpv_terminate_destroy(ptr);
        } else {
            mpv_destroy(ptr);
        }
    }
}

impl MpvHandle {
    fn ctx_ptr(&self) -> *mut c_void {
        self.ctx.load(Ordering::Acquire)
    }

    pub(crate) fn new(
        raw_window: *const c_void,
        display: Option<*const c_void>,
        mode: i32,
        app_handle: AppHandle,
    ) -> Result<Self, String> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        ensure_numeric_locale_for_mpv();

        let ctx = unsafe { mpv_create() };
        if ctx.is_null() {
            return Err(
                "Failed to create MPV context. Ensure libmpv is installed and LC_NUMERIC=C."
                    .into(),
            );
        }

        let set_str = |name: &str, value: &str| {
            let c_name = CString::new(name).unwrap();
            let c_value = CString::new(value).unwrap();
            unsafe { mpv_set_option_string(ctx, c_name.as_ptr(), c_value.as_ptr()) };
        };

        // soia_utils handles vo, wid, and GPU backend selection — only set
        // options that are independent of the render path.
        // Force Metal API on macOS to avoid MoltenVK/Vulkan shader compilation delays.
        #[cfg(target_os = "macos")]
        set_str("gpu-api", "metal");
        // Hardware decode. On Linux the bundled ffmpeg's hw paths (nvdec/vdpau)
        // segfault inside avcodec_send_packet on NVIDIA driver stacks, so
        // default to software decoding there; override with MAGNOLIA_HWDEC=auto
        // (or any other mpv hwdec value) to experiment.
        #[cfg(target_os = "linux")]
        {
            let hwdec = std::env::var("MAGNOLIA_HWDEC").unwrap_or_else(|_| "no".into());
            set_str("hwdec", &hwdec);
        }
        #[cfg(not(target_os = "linux"))]
        set_str("hwdec", "auto");
        set_str("osc", "no");
        set_str("osd-level", "0");
        set_str("demuxer-lavf-o", "reconnect=1,reconnect_streamed=1");
        // Aggressive demuxer cache for torrent streaming: mpv reading ahead on
        // the local HTTP stream is what pulls librqbit's piece-priority window
        // forward, so a deep readahead keeps the swarm busy ahead of playback.
        // The back-buffer lets short rewinds replay from RAM instead of
        // re-requesting ranges (which would reopen the torrent stream).
        set_str("cache", "yes");
        set_str("demuxer-max-bytes", "256MiB");
        set_str("demuxer-max-back-bytes", "64MiB");
        set_str("demuxer-readahead-secs", "600");
        // Start playback as soon as the first frames are decodable instead of
        // waiting for the cache to pre-fill.
        set_str("cache-pause-initial", "no");
        // Disable automatic subtitle selection so the UI and mpv stay in sync.
        // loadTrackPreferences() will apply any saved preference after file load.
        set_str("sub-auto", "no");
        // Default subtitles to the bundled Geist font. sub-fonts-dir needs
        // mpv >= 0.36; on older builds the option is unknown and the font
        // falls back to the system default.
        if let Some(fonts_dir) = seed_subtitle_fonts(&app_handle) {
            set_str("sub-fonts-dir", &fonts_dir.to_string_lossy());
            set_str("sub-font", "Geist");
        }

        let init_result = unsafe { mpv_initialize(ctx) };
        if init_result < 0 {
            unsafe { mpv_destroy(ctx) };
            return Err(format!("mpv_initialize failed (error {})", init_result));
        }

        // ── soia_utils: handles embedding on all platforms ─────────────────
        // mode (decided by the caller from the actual window handle type):
        // macOS = 0 (native/Metal), X11/HWND = 1 (wid), Wayland = 2 (render context).
        let display_ptr = display.unwrap_or(std::ptr::null());
        let soia_utils_ptr = unsafe {
            soia_utils_create(
                ctx,
                raw_window,
                display_ptr,
                mode,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if soia_utils_ptr.is_null() {
            unsafe { mpv_destroy(ctx) };
            return Err(
                "soia_utils_create failed — ensure libsoia_utils is present in src-tauri/libs/mpv/lib/"
                    .into(),
            );
        }

        let handle = MpvHandle {
            ctx: AtomicPtr::new(ctx),
            is_playing: Arc::new(AtomicBool::new(false)),
            eof_reached: Arc::new(AtomicBool::new(false)),
            app_handle,
            event_loop_stop: Arc::new(AtomicBool::new(false)),
            event_loop_handle: Mutex::new(None),
            render_loop_stop: Arc::new(AtomicBool::new(false)),
            render_loop_handle: Mutex::new(None),
            soia_utils: AtomicPtr::new(soia_utils_ptr),
        };

        Ok(handle)
    }

    pub fn create_client(&self, name: &str) -> Result<*mut c_void, String> {
        let c_name = CString::new(name).expect("null byte in client name");
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return Err("MPV context not initialised".to_string());
        }
        let client_ctx = unsafe { mpv_create_client(ctx, c_name.as_ptr()) };
        if client_ctx.is_null() {
            Err("Failed to create MPV client handle".to_string())
        } else {
            Ok(client_ctx)
        }
    }

    pub fn command(&self, args: &[&str]) -> c_int {
        let c_strings: Vec<CString> = args
            .iter()
            .map(|&s| CString::new(s).expect("null byte in arg"))
            .collect();
        let mut raw_args: Vec<*const c_char> =
            c_strings.iter().map(|s| s.as_ptr()).collect();
        raw_args.push(std::ptr::null());

        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe { mpv_command(ctx, raw_args.as_ptr()) }
    }

    pub fn set_option_int(&self, name: &str, value: i64) -> c_int {
        let c_name = CString::new(name).expect("null byte in name");
        let mut val = value;
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe {
            mpv_set_option(
                ctx,
                c_name.as_ptr(),
                mpv_format::MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        }
    }

    pub fn set_option_string(&self, name: &str, value: &str) -> i32 {
        let c_name = CString::new(name).expect("null byte in name");
        let c_value = CString::new(value).expect("null byte in value");
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return -1;
        }
        unsafe { mpv_set_option_string(ctx, c_name.as_ptr(), c_value.as_ptr()) as i32 }
    }

    pub fn get_property_string(&self, name: &str) -> Result<String, String> {
        let c_name = CString::new(name).map_err(|_| "Invalid property name".to_string())?;
        let ctx = self.ctx_ptr();
        if ctx.is_null() {
            return Err("MPV context not initialised".to_string());
        }
        unsafe {
            let value_ptr = mpv_get_property_string(ctx, c_name.as_ptr());
            if value_ptr.is_null() {
                return Err(format!("Failed to get MPV property: {name}"));
            }
            let value = CStr::from_ptr(value_ptr).to_string_lossy().into_owned();
            mpv_free(value_ptr as *mut c_void);
            Ok(value)
        }
    }

    /// Returns the raw soia_utils pointer (used by macOS layer geometry sync).
    #[cfg(target_os = "macos")]
    pub(crate) fn soia_utils_ptr(&self) -> *mut SoiaUtils {
        self.soia_utils.load(Ordering::Acquire)
    }

    /// Start the soia_utils render loop if the render context path is active.
    /// On native/wid modes (macOS mode-0, Windows/Linux mode-1) soia_utils drives
    /// its own presentation; no render thread is required.
    pub fn start_render_loop(&self) {
        let utils = self.soia_utils.load(Ordering::Acquire);
        if utils.is_null() {
            return;
        }
        let needs_loop = unsafe { soia_utils_uses_render_context(utils) != 0 };
        if !needs_loop {
            return;
        }

        self.render_loop_stop.store(false, Ordering::SeqCst);
        let stop_flag = self.render_loop_stop.clone();
        let utils_usize = utils as usize;

        let thread = std::thread::Builder::new()
            .name("mpv-soia-render".into())
            .spawn(move || {
                let utils = utils_usize as *mut SoiaUtils;
                while !stop_flag.load(Ordering::Relaxed) {
                    unsafe { soia_utils_render_context_update(utils) };
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
            })
            .expect("failed to spawn soia render thread");

        if let Ok(mut guard) = self.render_loop_handle.lock() {
            *guard = Some(thread);
        }
    }

    pub fn start_event_listener(&self) {
        self.stop_event_listener();
        self.event_loop_stop.store(false, Ordering::SeqCst);
        self.eof_reached.store(false, Ordering::SeqCst);
        let app_handle_clone = self.app_handle.clone();
        let stop_flag = self.event_loop_stop.clone();
        let is_playing = self.is_playing.clone();
        let eof_reached = self.eof_reached.clone();
        let handle = std::thread::spawn(move || {
            mpv_event_loop(app_handle_clone, stop_flag, is_playing, eof_reached)
        });
        if let Ok(mut guard) = self.event_loop_handle.lock() {
            *guard = Some(handle);
        }
    }

    pub fn stop_event_listener(&self) {
        self.event_loop_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.event_loop_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        self.eof_reached.store(false, Ordering::SeqCst);
    }

    fn stop_render_loop(&self) {
        self.render_loop_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.render_loop_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    fn free_soia_utils(&self) {
        let utils = self.soia_utils.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !utils.is_null() {
            unsafe { soia_utils_destroy(utils) };
        }
    }

    pub fn eof_reached(&self) -> bool {
        self.eof_reached.load(Ordering::Acquire)
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::Relaxed)
    }

    /// Notify soia_utils that the window was resized (physical pixel dimensions).
    pub fn resize_view(&self, logical_w: f64, logical_h: f64, scale: f64) {
        let utils = self.soia_utils.load(Ordering::Acquire);
        if !utils.is_null() {
            let pw = (logical_w * scale) as u32;
            let ph = (logical_h * scale) as u32;
            unsafe { soia_utils_render_target_resize(utils, pw, ph) };
        }
    }

    pub fn terminate(&self) {
        self.stop_event_listener();
        self.stop_render_loop();
        // soia_utils must be freed before mpv context.
        self.free_soia_utils();
        let ctx = self.ctx.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !ctx.is_null() {
            info!("Terminating MPV player handle...");
            unsafe { mpv_terminate_destroy(ctx) };
        }
    }
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        self.stop_event_listener();
        self.stop_render_loop();
        self.free_soia_utils();
        release_mpv_context(&self.ctx, false);
    }
}



