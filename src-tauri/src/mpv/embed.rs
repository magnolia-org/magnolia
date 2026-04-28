/// Platform-specific embedding of libmpv's render output into the native window,
/// positioned *below* the transparent Tauri WebView so the WebView UI overlays mpv.
///
/// Returns an `EmbedHandle` containing the `wid` integer to pass to mpv's `wid` option.
/// On Wayland, `uses_render_context` is true and the render loop must be driven externally.
use std::ffi::c_void;

pub struct EmbedHandle {
    /// The window-ID integer to pass to mpv via `mpv_set_option_int("wid", wid)`.
    /// On render-context paths this value is unused; set to 0.
    pub wid: i64,
    /// If true, the caller must spawn a render loop that calls `render_context_update`.
    pub uses_render_context: bool,
    /// Platform-specific drop guard to keep embedded resources alive.
    _platform: PlatformEmbed,
}

enum PlatformEmbed {
    #[cfg(target_os = "windows")]
    Windows(WindowsEmbed),
    #[cfg(target_os = "linux")]
    Linux(LinuxEmbed),
    #[allow(dead_code)]
    None,
}

// ──────────────────────────────────────────────────────────────────────────
// macOS — soia_utils handles all embedding; no sub-view needed.
//
// soia_utils_create() (called from handle.rs) receives the WKWebView's own
// NSView pointer and installs its own Metal/Vulkan layer inside it.
// We simply return a no-op EmbedHandle so handle.rs skips the wid option.
// ──────────────────────────────────────────────────────────────────────────
#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;

    pub(super) unsafe fn macos_setup_window(
        _ns_view_ptr: *const c_void,
    ) -> Result<EmbedHandle, String> {
        Ok(EmbedHandle {
            wid: 0,
            // Prevents handle.rs from setting the `wid` mpv option.
            // soia_utils handles all embedding via the raw_window pointer.
            uses_render_context: true,
            _platform: PlatformEmbed::None,
        })
    }
}

// ── Public helpers ────────────────────────────────────────────────────────

impl EmbedHandle {
    /// Resize notification — not needed on macOS because soia_utils_render_target_resize
    /// is called directly from handle.rs. No-op here; kept for API symmetry.
    pub fn resize(&self, _logical_w: f64, _logical_h: f64, _scale: f64) {}
}

// ──────────────────────────────────────────────────────────────────────────
// Windows — create a child HWND below WebView2, pass it as wid
// ──────────────────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
struct WindowsEmbed {
    child_hwnd: isize,
}

#[cfg(target_os = "windows")]
unsafe impl Send for WindowsEmbed {}
#[cfg(target_os = "windows")]
unsafe impl Sync for WindowsEmbed {}

#[cfg(target_os = "windows")]
impl Drop for WindowsEmbed {
    fn drop(&mut self) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        if self.child_hwnd != 0 {
            unsafe {
                let _ = DestroyWindow(HWND(self.child_hwnd));
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::{HINSTANCE, HWND};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GetClientRect, SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, WS_CHILD, WS_VISIBLE, WINDOW_EX_STYLE,
    };

    pub(super) unsafe fn windows_setup_window(
        hwnd_ptr: *const c_void,
    ) -> Result<EmbedHandle, String> {
        let parent = HWND(hwnd_ptr as isize);

        // Get client area size.
        let mut rect = windows::Win32::Foundation::RECT::default();
        GetClientRect(parent, &mut rect).map_err(|e| format!("GetClientRect failed: {e}"))?;
        let w = (rect.right - rect.left) as i32;
        let h = (rect.bottom - rect.top) as i32;

        let hinstance: HINSTANCE = GetModuleHandleW(None)
            .map_err(|e| format!("GetModuleHandleW failed: {e}"))?
            .into();

        // Create a plain child window that mpv will render into.
        let child = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            w,
            h,
            parent,
            None,
            hinstance,
            None,
        )
        .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

        // Push it behind WebView2.
        SetWindowPos(child, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
            .map_err(|e| format!("SetWindowPos failed: {e}"))?;

        let wid = child.0 as i64;
        Ok(EmbedHandle {
            wid,
            uses_render_context: false,
            _platform: PlatformEmbed::Windows(WindowsEmbed { child_hwnd: child.0 }),
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Linux — X11 child window or Wayland render-context
// ──────────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
struct LinuxEmbed {
    /// Non-zero if we created an X11 child window that must be destroyed on drop.
    x11_child_xid: u32,
    /// Connection handle used to destroy the window on drop.
    x11_conn: Option<std::sync::Arc<x11rb::xcb_ffi::XCBConnection>>,
}

#[cfg(target_os = "linux")]
unsafe impl Send for LinuxEmbed {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LinuxEmbed {}

#[cfg(target_os = "linux")]
impl Drop for LinuxEmbed {
    fn drop(&mut self) {
        if self.x11_child_xid != 0 {
            if let Some(conn) = &self.x11_conn {
                use x11rb::connection::Connection;
                use x11rb::protocol::xproto::ConnectionExt;
                let _ = conn.destroy_window(self.x11_child_xid);
                let _ = conn.flush();
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ConfigureWindowAux, ConnectionExt, CreateWindowAux, EventMask, StackMode, WindowClass,
        COPY_DEPTH_FROM_PARENT,
    };
    use x11rb::xcb_ffi::XCBConnection;

    pub(super) unsafe fn linux_setup_x11(
        parent_xid: u32,
        display_ptr: Option<*const c_void>,
    ) -> Result<EmbedHandle, String> {
        // Connect to the X11 server. If a display pointer was provided, use it;
        // otherwise fall back to $DISPLAY.
        let (conn, screen_num) = if let Some(dpy) = display_ptr {
            // Wrap the existing xcb_connection_t obtained from GTK/GDK.
            XCBConnection::from_raw_xcb_connection(dpy as *mut _, false)
                .map_err(|e| format!("XCBConnection from display failed: {e}"))?
        } else {
            XCBConnection::connect(None)
                .map(|(conn, screen)| (conn, screen))
                .map_err(|e| format!("X11 connect failed: {e}"))?
        };
        let conn = Arc::new(conn);

        let screen = &conn.setup().roots[screen_num];
        let root_depth = screen.root_depth;
        let visual = screen.root_visual;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;

        let child_xid = conn.generate_id().map_err(|e| format!("generate_id: {e}"))?;
        conn.create_window(
            root_depth,
            child_xid,
            parent_xid,
            0,
            0,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(0)
                .event_mask(EventMask::EXPOSURE),
        )
        .map_err(|e| format!("create_window: {e}"))?;

        conn.map_window(child_xid)
            .map_err(|e| format!("map_window: {e}"))?;

        // Stack it below the GTK/WebkitGTK surface.
        conn.configure_window(
            child_xid,
            &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
        )
        .map_err(|e| format!("configure_window: {e}"))?;

        conn.flush().map_err(|e| format!("x11 flush: {e}"))?;

        Ok(EmbedHandle {
            wid: child_xid as i64,
            uses_render_context: false,
            _platform: PlatformEmbed::Linux(LinuxEmbed {
                x11_child_xid: child_xid,
                x11_conn: Some(conn),
            }),
        })
    }

    /// Wayland path: we cannot embed via wid; use mpv render context instead.
    pub(super) fn linux_setup_wayland() -> EmbedHandle {
        EmbedHandle {
            wid: 0,
            uses_render_context: true,
            _platform: PlatformEmbed::Linux(LinuxEmbed {
                x11_child_xid: 0,
                x11_conn: None,
            }),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────

/// Set up the mpv render target for the given platform window handle.
///
/// * `raw_window` — on macOS: `NSView*` (WKWebView); on Windows: parent `HWND`;
///   on Linux X11: parent `XID` cast to pointer.
/// * `display` — on Linux only: the `xcb_connection_t*` from GDK, or `None`
///   to let x11rb open a new connection from `$DISPLAY`.
pub fn setup_mpv_window(
    raw_window: *const c_void,
    display: Option<*const c_void>,
) -> Result<EmbedHandle, String> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            unsafe { macos_impl::macos_setup_window(raw_window) }
        } else if #[cfg(target_os = "windows")] {
            unsafe { windows_impl::windows_setup_window(raw_window) }
        } else if #[cfg(target_os = "linux")] {
            let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
            if session.trim().eq_ignore_ascii_case("wayland") {
                Ok(linux_impl::linux_setup_wayland())
            } else {
                // X11 or unknown — treat as X11.
                let parent_xid = raw_window as u32;
                unsafe { linux_impl::linux_setup_x11(parent_xid, display) }
            }
        } else {
            let _ = (raw_window, display);
            Err("mpv embed: unsupported platform".to_string())
        }
    }
}
