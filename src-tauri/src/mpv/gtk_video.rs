//! X11 video embedding: mpv renders into a GTK widget placed under the
//! (transparent) webview inside a `GtkOverlay`, so the window itself never
//! needs a foreign child X window.
//!
//! Two backends:
//! - **OpenGL** (default): a `GtkGLArea` driven by mpv's OpenGL render API.
//!   Scaling, colour conversion and (when enabled) hwdec interop all run on
//!   the GPU, and GTK composites the area's texture without a CPU readback.
//! - **Software**: a `GtkDrawingArea` fed by mpv's software render API and
//!   blitted with cairo. Used when GL is unavailable or when forced with
//!   `MAGNOLIA_VIDEO_RENDER=sw`.

use super::ffi::{
    mpv_render_context_create, mpv_render_context_free, mpv_render_context_render,
    mpv_render_context_report_swap, mpv_render_context_set_update_callback,
    mpv_render_context_update, MpvOpenGlFbo, MpvOpenGlInitParams, MpvRenderParam,
    MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_FLIP_Y, MPV_RENDER_PARAM_INVALID,
    MPV_RENDER_PARAM_OPENGL_FBO, MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
    MPV_RENDER_PARAM_SW_FORMAT, MPV_RENDER_PARAM_SW_POINTER, MPV_RENDER_PARAM_SW_SIZE,
    MPV_RENDER_PARAM_SW_STRIDE, MPV_RENDER_PARAM_X11_DISPLAY, MPV_RENDER_UPDATE_FRAME,
};
use gtk::cairo;
use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void, CStr};
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use webkit2gtk::WebViewExt;

pub const EMBED_MODE_GTK: i32 = 3;

/// True while the software backend is active (the UI then offers CPU
/// scaler presets, which have no effect on the GL backend).
static SOFTWARE_RENDERING: AtomicBool = AtomicBool::new(false);

/// Set once mpv has been embedded through this module (X11 only).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// GL_RENDERER string of the context mpv renders with (shown in the stats).
static GL_RENDERER_NAME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// GL symbol loader chosen at realize time (GLX or EGL), as a fn pointer.
static GL_LOADER: AtomicUsize = AtomicUsize::new(0);

type GlLoaderFn = unsafe extern "C" fn(name: *const c_char) -> *mut c_void;
type GlGetIntegervFn = unsafe extern "C" fn(pname: u32, data: *mut c_int);
type GlGetStringFn = unsafe extern "C" fn(name: u32) -> *const c_char;

const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_RENDERER: u32 = 0x1F01;

#[derive(Clone, Copy, PartialEq)]
enum Backend {
    Gl,
    Software,
}

thread_local! {
    static OVERLAY: RefCell<Option<gtk::Overlay>> = const { RefCell::new(None) };
    static VIDEO: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
    static BACKEND: Cell<Backend> = const { Cell::new(Backend::Software) };
    static MPV: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static RENDER_CTX: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static GL_GET_INTEGERV: Cell<Option<GlGetIntegervFn>> = const { Cell::new(None) };
    static SURFACE: RefCell<Option<cairo::ImageSurface>> = const { RefCell::new(None) };
    static FRAME_W: Cell<i32> = const { Cell::new(0) };
    static NEEDS_RENDER: Cell<bool> = const { Cell::new(true) };
}

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

pub fn software_rendering() -> bool {
    SOFTWARE_RENDERING.load(Ordering::Acquire)
}

/// GPU name reported by the GL context, while the OpenGL backend is active.
pub fn gl_renderer_name() -> Option<String> {
    if software_rendering() {
        return None;
    }
    GL_RENDERER_NAME.lock().ok()?.clone()
}

pub fn install(window: &tauri::WebviewWindow, mpv_ctx: *mut c_void) -> Result<(), String> {
    let mpv = mpv_ctx as usize;
    let win = window.clone();
    window
        .with_webview(move |platform| {
            let vbox = match win.default_vbox() {
                Ok(b) => b,
                Err(e) => {
                    log::error!("[gtk-video] no default vbox: {e}");
                    return;
                }
            };
            attach(&vbox, &platform.inner(), mpv as *mut c_void);
        })
        .map_err(|e| e.to_string())
}

fn attach(vbox: &gtk::Box, webview: &webkit2gtk::WebView, mpv: *mut c_void) {
    let webview_widget: gtk::Widget = webview.clone().upcast();
    let Some(position) = vbox.children().iter().position(|c| c == &webview_widget) else {
        log::error!("[gtk-video] webview is not a direct child of the window vbox");
        return;
    };
    MPV.set(mpv);
    INSTALLED.store(true, Ordering::Release);

    let overlay = gtk::Overlay::new();
    vbox.remove(&webview_widget);
    overlay.add_overlay(&webview_widget);
    vbox.pack_start(&overlay, true, true, 0);
    vbox.reorder_child(&overlay, position as i32);
    webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    OVERLAY.with(|o| *o.borrow_mut() = Some(overlay.clone()));

    let forced_sw = std::env::var("MAGNOLIA_VIDEO_RENDER").is_ok_and(|v| v.eq_ignore_ascii_case("sw"));
    let video = if forced_sw { build_sw_area() } else { build_gl_area() };
    set_video_widget(video);
    overlay.show_all();
}

/// Places `widget` as the overlay's base child, replacing any previous one.
fn set_video_widget(widget: gtk::Widget) {
    OVERLAY.with(|o| {
        let Some(overlay) = o.borrow().clone() else { return };
        VIDEO.with(|v| {
            if let Some(old) = v.borrow_mut().take() {
                overlay.remove(&old);
            }
            widget.set_hexpand(true);
            widget.set_vexpand(true);
            overlay.add(&widget);
            widget.show();
            *v.borrow_mut() = Some(widget);
        });
    });
}

fn fall_back_to_software(reason: &str) {
    log::warn!("[gtk-video] OpenGL video unavailable ({reason}); using software rendering");
    // Swapping widgets from inside the GL area's own realize handler is not
    // safe, so do it on the next main-loop iteration.
    glib::idle_add_local_once(|| set_video_widget(build_sw_area()));
}

// ── OpenGL backend ───────────────────────────────────────────────────────────

fn build_gl_area() -> gtk::Widget {
    BACKEND.set(Backend::Gl);
    SOFTWARE_RENDERING.store(false, Ordering::Release);

    let area = gtk::GLArea::new();
    // mpv only redraws when it has a new frame; webview repaints above the
    // area reuse the last rendered texture instead of re-rendering video.
    area.set_auto_render(false);
    area.set_has_alpha(false);
    area.set_has_depth_buffer(false);
    area.set_has_stencil_buffer(false);

    area.connect_realize(|area| {
        area.make_current();
        if let Some(err) = area.error() {
            fall_back_to_software(&err.to_string());
            return;
        }
        if let Err(reason) = create_gl_render_context() {
            fall_back_to_software(&reason);
            return;
        }
        log::info!("[gtk-video] mpv video embedded under the webview (OpenGL render)");
    });
    area.connect_unrealize(|area| {
        // After a fallback the software backend owns RENDER_CTX already.
        if BACKEND.get() == Backend::Gl {
            area.make_current();
            free_render_context();
        }
    });
    area.connect_render(|area, _ctx| {
        render_gl(area);
        glib::Propagation::Stop
    });
    area.upcast()
}

unsafe fn dlsym_default(name: &CStr) -> *mut c_void {
    libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr())
}

/// Picks the symbol loader matching the GL context GTK made current.
fn select_gl_loader() -> Result<(), String> {
    unsafe {
        type EglGetCurrentContext = unsafe extern "C" fn() -> *mut c_void;
        let egl_current = dlsym_default(c"eglGetCurrentContext");
        let using_egl = !egl_current.is_null() && {
            let f: EglGetCurrentContext = std::mem::transmute(egl_current);
            !f().is_null()
        };
        let loader = if using_egl {
            dlsym_default(c"eglGetProcAddress")
        } else {
            let arb = dlsym_default(c"glXGetProcAddressARB");
            if arb.is_null() { dlsym_default(c"glXGetProcAddress") } else { arb }
        };
        if loader.is_null() {
            return Err("no GL symbol loader".into());
        }
        GL_LOADER.store(loader as usize, Ordering::Release);
    }
    Ok(())
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let loader = GL_LOADER.load(Ordering::Acquire);
    if loader != 0 {
        let f: GlLoaderFn = std::mem::transmute(loader);
        let p = f(name);
        if !p.is_null() {
            return p;
        }
    }
    libc::dlsym(libc::RTLD_DEFAULT, name)
}

/// The Xlib `Display*` behind GDK, so mpv can set up vaapi/vdpau interop.
fn x11_display() -> *mut c_void {
    let Some(display) = gtk::gdk::Display::default() else {
        return std::ptr::null_mut();
    };
    if display.type_().name() != "GdkX11Display" {
        return std::ptr::null_mut();
    }
    unsafe {
        type GetXDisplay = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
        let sym = dlsym_default(c"gdk_x11_display_get_xdisplay");
        if sym.is_null() {
            return std::ptr::null_mut();
        }
        let f: GetXDisplay = std::mem::transmute(sym);
        let raw: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0;
        f(raw.cast())
    }
}

fn create_gl_render_context() -> Result<(), String> {
    select_gl_loader()?;
    let get_integerv = unsafe { get_proc_address(std::ptr::null_mut(), c"glGetIntegerv".as_ptr()) };
    if get_integerv.is_null() {
        return Err("glGetIntegerv not found".into());
    }
    GL_GET_INTEGERV.set(Some(unsafe { std::mem::transmute::<*mut c_void, GlGetIntegervFn>(get_integerv) }));
    let get_string = unsafe { get_proc_address(std::ptr::null_mut(), c"glGetString".as_ptr()) };
    if !get_string.is_null() {
        let get_string: GlGetStringFn = unsafe { std::mem::transmute(get_string) };
        let name = unsafe { get_string(GL_RENDERER) };
        if !name.is_null() {
            let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
            if let Ok(mut slot) = GL_RENDERER_NAME.lock() {
                *slot = Some(name);
            }
        }
    }

    let mut init = MpvOpenGlInitParams {
        get_proc_address: Some(get_proc_address),
        get_proc_address_ctx: std::ptr::null_mut(),
    };
    let xdisplay = x11_display();
    let mut params = vec![
        MpvRenderParam { ty: MPV_RENDER_PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
        MpvRenderParam {
            ty: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            data: &mut init as *mut MpvOpenGlInitParams as *mut c_void,
        },
    ];
    if !xdisplay.is_null() {
        params.push(MpvRenderParam { ty: MPV_RENDER_PARAM_X11_DISPLAY, data: xdisplay });
    }
    params.push(MpvRenderParam { ty: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() });

    let mut ctx: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { mpv_render_context_create(&mut ctx, MPV.get(), params.as_mut_ptr()) };
    if rc < 0 || ctx.is_null() {
        return Err(format!("mpv_render_context_create(opengl) failed: {rc}"));
    }
    unsafe { mpv_render_context_set_update_callback(ctx, Some(on_mpv_update), std::ptr::null_mut()) };
    RENDER_CTX.set(ctx);
    Ok(())
}

fn render_gl(area: &gtk::GLArea) {
    let ctx = RENDER_CTX.get();
    let Some(get_integerv) = GL_GET_INTEGERV.get() else { return };
    if ctx.is_null() {
        return;
    }
    let scale = area.scale_factor();
    let w = area.allocated_width() * scale;
    let h = area.allocated_height() * scale;
    if w <= 0 || h <= 0 {
        return;
    }
    // GtkGLArea binds its own FBO before emitting ::render.
    let mut fbo: c_int = 0;
    unsafe { get_integerv(GL_FRAMEBUFFER_BINDING, &mut fbo) };
    let mut target = MpvOpenGlFbo { fbo, w, h, internal_format: 0 };
    let mut flip_y: c_int = 1;
    let mut params = [
        MpvRenderParam {
            ty: MPV_RENDER_PARAM_OPENGL_FBO,
            data: &mut target as *mut MpvOpenGlFbo as *mut c_void,
        },
        MpvRenderParam { ty: MPV_RENDER_PARAM_FLIP_Y, data: &mut flip_y as *mut c_int as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    let rc = unsafe { mpv_render_context_render(ctx, params.as_mut_ptr()) };
    if rc < 0 {
        log::warn!("[gtk-video] mpv_render_context_render(opengl) failed: {rc}");
    }
    unsafe { mpv_render_context_report_swap(ctx) };
}

// ── shared ───────────────────────────────────────────────────────────────────

fn free_render_context() {
    let ctx = RENDER_CTX.replace(std::ptr::null_mut());
    if !ctx.is_null() {
        unsafe {
            mpv_render_context_set_update_callback(ctx, None, std::ptr::null_mut());
            mpv_render_context_free(ctx);
        }
    }
    SURFACE.with(|s| s.borrow_mut().take());
}

unsafe extern "C" fn on_mpv_update(_ctx: *mut c_void) {
    glib::MainContext::default().invoke(|| {
        let ctx = RENDER_CTX.get();
        if ctx.is_null() {
            return;
        }
        let flags = unsafe { mpv_render_context_update(ctx) };
        if flags & MPV_RENDER_UPDATE_FRAME == 0 {
            return;
        }
        VIDEO.with(|v| {
            let Some(widget) = v.borrow().clone() else { return };
            match BACKEND.get() {
                Backend::Gl => {
                    if let Some(area) = widget.downcast_ref::<gtk::GLArea>() {
                        area.queue_render();
                    }
                }
                Backend::Software => {
                    NEEDS_RENDER.set(true);
                    widget.queue_draw();
                }
            }
        });
    });
}

// ── software backend ─────────────────────────────────────────────────────────

fn build_sw_area() -> gtk::Widget {
    // Any GL context left from a failed GL realize can't be freed safely
    // without its GL context current; realize failures leave it null anyway.
    BACKEND.set(Backend::Software);
    if !create_sw_render_context() {
        return gtk::DrawingArea::new().upcast();
    }
    SOFTWARE_RENDERING.store(true, Ordering::Release);

    let area = gtk::DrawingArea::new();
    area.connect_draw(|area, cr| {
        draw_sw(area, cr);
        glib::Propagation::Stop
    });
    area.connect_destroy(|_| free_render_context());
    log::info!("[gtk-video] mpv video embedded under the webview (software render)");
    area.upcast()
}

fn create_sw_render_context() -> bool {
    let mut params = [
        MpvRenderParam { ty: MPV_RENDER_PARAM_API_TYPE, data: c"sw".as_ptr() as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    let mut ctx: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { mpv_render_context_create(&mut ctx, MPV.get(), params.as_mut_ptr()) };
    if rc < 0 || ctx.is_null() {
        log::error!("[gtk-video] mpv_render_context_create(sw) failed: {rc}");
        return false;
    }
    unsafe { mpv_render_context_set_update_callback(ctx, Some(on_mpv_update), std::ptr::null_mut()) };
    RENDER_CTX.set(ctx);
    NEEDS_RENDER.set(true);
    true
}

fn draw_sw(area: &gtk::DrawingArea, cr: &cairo::Context) {
    let scale = area.scale_factor();
    let w = area.allocated_width() * scale;
    let h = area.allocated_height() * scale;
    if w <= 0 || h <= 0 {
        return;
    }
    SURFACE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let size_ok = slot.as_ref().is_some_and(|s| s.height() == h) && FRAME_W.get() == w;
        if !size_ok {
            let padded = (w + 15) & !15;
            *slot = cairo::ImageSurface::create(cairo::Format::Rgb24, padded, h).ok();
            FRAME_W.set(w);
            NEEDS_RENDER.set(true);
        }
        let Some(surface) = slot.as_mut() else { return };
        if NEEDS_RENDER.replace(false) {
            render_sw(surface, w, h);
        }
        cr.save().ok();
        cr.scale(1.0 / scale as f64, 1.0 / scale as f64);
        cr.set_source_surface(&**surface, 0.0, 0.0).ok();
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        cr.fill().ok();
        cr.restore().ok();
    });
}

fn render_sw(surface: &mut cairo::ImageSurface, w: i32, h: i32) {
    let ctx = RENDER_CTX.get();
    if ctx.is_null() {
        return;
    }
    let mut stride = surface.stride() as usize;
    let Ok(mut data) = surface.data() else {
        log::warn!("[gtk-video] frame surface is shared; skipping render");
        return;
    };
    let mut size: [c_int; 2] = [w, h];
    let mut params = [
        MpvRenderParam { ty: MPV_RENDER_PARAM_SW_SIZE, data: size.as_mut_ptr() as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_SW_FORMAT, data: c"bgr0".as_ptr() as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_SW_STRIDE, data: &mut stride as *mut usize as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_SW_POINTER, data: data.as_mut_ptr() as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    let rc = unsafe { mpv_render_context_render(ctx, params.as_mut_ptr()) };
    if rc < 0 {
        log::warn!("[gtk-video] mpv_render_context_render(sw) failed: {rc}");
    }
}
