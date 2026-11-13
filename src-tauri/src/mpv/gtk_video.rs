use super::ffi::{
    mpv_render_context_create, mpv_render_context_free, mpv_render_context_render,
    mpv_render_context_set_update_callback, mpv_render_context_update, MpvRenderParam,
    MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_INVALID, MPV_RENDER_PARAM_SW_FORMAT,
    MPV_RENDER_PARAM_SW_POINTER, MPV_RENDER_PARAM_SW_SIZE, MPV_RENDER_PARAM_SW_STRIDE,
    MPV_RENDER_UPDATE_FRAME,
};
use gtk::cairo;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::os::raw::c_int;
use webkit2gtk::WebViewExt;

pub const EMBED_MODE_GTK: i32 = 3;

thread_local! {
    static AREA: RefCell<Option<gtk::DrawingArea>> = const { RefCell::new(None) };
    static RENDER_CTX: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static SURFACE: RefCell<Option<cairo::ImageSurface>> = const { RefCell::new(None) };
    static FRAME_W: Cell<i32> = const { Cell::new(0) };
    static NEEDS_RENDER: Cell<bool> = const { Cell::new(true) };
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

    if !create_render_context(mpv) {
        return;
    }

    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.connect_draw(|area, cr| {
        draw(area, cr);
        glib::Propagation::Stop
    });
    area.connect_destroy(|_| free_render_context());

    let overlay = gtk::Overlay::new();
    vbox.remove(&webview_widget);
    overlay.add(&area);
    overlay.add_overlay(&webview_widget);
    vbox.pack_start(&overlay, true, true, 0);
    vbox.reorder_child(&overlay, position as i32);

    webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));

    AREA.with(|a| *a.borrow_mut() = Some(area));
    overlay.show_all();
    log::info!("[gtk-video] mpv video embedded under the webview (software render)");
}

fn create_render_context(mpv: *mut c_void) -> bool {
    let mut params = [
        MpvRenderParam { ty: MPV_RENDER_PARAM_API_TYPE, data: c"sw".as_ptr() as *mut c_void },
        MpvRenderParam { ty: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
    ];
    let mut ctx: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { mpv_render_context_create(&mut ctx, mpv, params.as_mut_ptr()) };
    if rc < 0 || ctx.is_null() {
        log::error!("[gtk-video] mpv_render_context_create failed: {rc}");
        return false;
    }
    unsafe { mpv_render_context_set_update_callback(ctx, Some(on_mpv_update), std::ptr::null_mut()) };
    RENDER_CTX.set(ctx);
    true
}

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
        if flags & MPV_RENDER_UPDATE_FRAME != 0 {
            NEEDS_RENDER.set(true);
            AREA.with(|a| {
                if let Some(area) = a.borrow().as_ref() {
                    area.queue_draw();
                }
            });
        }
    });
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context) {
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
            render_into(surface, w, h);
        }
        cr.save().ok();
        cr.scale(1.0 / scale as f64, 1.0 / scale as f64);
        cr.set_source_surface(&**surface, 0.0, 0.0).ok();
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        cr.fill().ok();
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.restore().ok();
    });
}

fn render_into(surface: &mut cairo::ImageSurface, w: i32, h: i32) {
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
        log::warn!("[gtk-video] mpv_render_context_render failed: {rc}");
    }
}
