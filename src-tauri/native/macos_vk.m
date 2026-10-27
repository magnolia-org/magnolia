#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

// vulkan (only the parts used here)
typedef struct VkInstance_T *VkInstance;
typedef uint64_t VkSurfaceKHR;
typedef int32_t VkResult;
typedef void (*PFN_vkVoidFunction)(void);
typedef PFN_vkVoidFunction (*PFN_vkGetInstanceProcAddr)(VkInstance, const char *);
typedef struct {
    int32_t sType;
    const void *pNext;
    uint32_t flags;
    const void *pLayer;
} VkMetalSurfaceCreateInfoEXT;
typedef VkResult (*PFN_vkCreateMetalSurfaceEXT)(VkInstance, const VkMetalSurfaceCreateInfoEXT *,
                                                const void *, VkSurfaceKHR *);
typedef void (*PFN_vkDestroySurfaceKHR)(VkInstance, VkSurfaceKHR, const void *);
#define VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT 1000217000
#define VK_PRESENT_MODE_FIFO_KHR 2

// libplacebo (API 349)
enum pl_log_level { PL_LOG_NONE = 0, PL_LOG_FATAL, PL_LOG_ERR, PL_LOG_WARN, PL_LOG_INFO };
struct pl_log_params {
    void (*log_cb)(void *log_priv, enum pl_log_level level, const char *msg);
    void *log_priv;
    enum pl_log_level log_level;
};
typedef const struct pl_log_t *pl_log;
typedef const struct pl_vk_inst_t {
    VkInstance instance;
    uint32_t api_version;
    PFN_vkGetInstanceProcAddr get_proc_addr;
    const char *const *extensions;
    int num_extensions;
    const char *const *layers;
    int num_layers;
} *pl_vk_inst;
struct pl_vk_inst_params {
    bool debug;
    bool debug_extra;
    uint32_t max_api_version;
    PFN_vkGetInstanceProcAddr get_proc_addr;
    const char *const *extensions;
    int num_extensions;
    const char *const *opt_extensions;
    int num_opt_extensions;
    const char *const *layers;
    int num_layers;
    const char *const *opt_layers;
    int num_opt_layers;
};
typedef const struct pl_swapchain_t *pl_swapchain;
pl_log pl_log_create_349(int api_ver, const struct pl_log_params *params);
void pl_log_destroy(pl_log *log);
pl_vk_inst pl_vk_inst_create(pl_log log, const struct pl_vk_inst_params *params);
void pl_vk_inst_destroy(pl_vk_inst *inst);
bool pl_swapchain_resize(pl_swapchain sw, int *width, int *height);

// mpv v0.41.0
struct mpvk_ctx {
    pl_log pllog;
    pl_vk_inst vkinst;
    void *vulkan;
    void *gpu;
    pl_swapchain swapchain;
    VkSurfaceKHR surface;
};
struct ra_ctx_opts {
    bool allow_sw;
    bool want_alpha;
    bool debug;
    bool probing;
    bool composition;
    void *context_list;
    void *context_type_list;
};
struct ra_ctx {
    void *vo;
    void *ra;
    void *global;
    void *log;
    struct ra_ctx_opts opts;
    const struct ra_ctx_fns *fns;
    void *swapchain;
    void *spirv;
    void *priv;
};
struct ra_ctx_fns {
    const char *type;
    const char *name;
    const char *description;
    bool (*reconfig)(struct ra_ctx *ctx);
    bool (*pass_colorspace)(struct ra_ctx *ctx);
    int (*control)(struct ra_ctx *ctx, int *events, int request, void *arg);
    void (*wakeup)(struct ra_ctx *ctx);
    void (*wait_events)(struct ra_ctx *ctx, int64_t until_time_ns);
    void (*update_render_opts)(struct ra_ctx *ctx);
    bool (*init)(struct ra_ctx *ctx);
    void (*uninit)(struct ra_ctx *ctx);
};
struct ra_ctx_params {
    bool (*check_visible)(struct ra_ctx *ctx);
    int (*color_depth)(struct ra_ctx *ctx);
    void *preferred_csp;
    void (*get_vsync)(struct ra_ctx *ctx, void *info);
    void (*swap_buffers)(struct ra_ctx *ctx);
};
bool ra_vk_ctx_init(struct ra_ctx *ctx, struct mpvk_ctx *vk, struct ra_ctx_params params,
                    int preferred_mode);
void ra_vk_ctx_uninit(struct ra_ctx *ctx);

// struct vo: int dwidth, dheight live at 0xf8 (see ra_vk_ctx_resize in libmpv)
#define VO_DWIDTH_OFFSET 0xf8
#define VO_EVENT_RESIZE (1 << 1)
#define VOCTRL_CHECK_EVENTS 2
#define VO_NOTIMPL -3

// moltenvk sets the drawable size to 1x1 to force a presentation to complete, which can leave
// it stuck there and flicker (same workaround as mpv's MetalLayer)
@interface MagnoliaMetalLayer : CAMetalLayer
@end

@implementation MagnoliaMetalLayer
- (void)setDrawableSize:(CGSize)size
{
    if (size.width > 1 && size.height > 1)
        [super setDrawableSize:size];
}
@end

// layer state shared with the app (set on the main thread, read by the vo thread)
static CAMetalLayer *g_layer;
static _Atomic int g_pixel_w = 1;
static _Atomic int g_pixel_h = 1;

struct priv {
    struct mpvk_ctx vk;
    int w, h;
    bool edr;
    CGColorSpaceRef colorspace;
};

static void log_cb(void *priv, enum pl_log_level level, const char *msg)
{
    (void)priv;
    (void)level;
    fprintf(stderr, "[magnolia-vk] %s\n", msg);
}

static bool do_resize(struct ra_ctx *ctx)
{
    struct priv *p = ctx->priv;
    int w = atomic_load(&g_pixel_w);
    int h = atomic_load(&g_pixel_h);
    bool ok = pl_swapchain_resize(p->vk.swapchain, &w, &h);
    int *dsize = (int *)((char *)ctx->vo + VO_DWIDTH_OFFSET);
    dsize[0] = w;
    dsize[1] = h;
    p->w = atomic_load(&g_pixel_w);
    p->h = atomic_load(&g_pixel_h);
    return ok;
}

// moltenvk switches the layer to hdr (colorspace + edr) from the vo thread, which has no run
// loop, so the change only showed up once the main thread committed a transaction (e.g. on a
// resize). flush it as soon as it happens.
static void swap_buffers(struct ra_ctx *ctx)
{
    struct priv *p = ctx->priv;
    bool edr = g_layer.wantsExtendedDynamicRangeContent;
    CGColorSpaceRef colorspace = g_layer.colorspace;
    if (edr != p->edr || colorspace != p->colorspace) {
        p->edr = edr;
        p->colorspace = colorspace;
        [CATransaction flush];
    }
}

static void uninit(struct ra_ctx *ctx)
{
    struct priv *p = ctx->priv;
    if (!p)
        return;
    ra_vk_ctx_uninit(ctx);
    struct mpvk_ctx *vk = &p->vk;
    if (vk->surface && vk->vkinst) {
        PFN_vkDestroySurfaceKHR destroy =
            (PFN_vkDestroySurfaceKHR)vk->vkinst->get_proc_addr(vk->vkinst->instance,
                                                               "vkDestroySurfaceKHR");
        if (destroy)
            destroy(vk->vkinst->instance, vk->surface, NULL);
        vk->surface = 0;
    }
    pl_vk_inst_destroy(&vk->vkinst);
    pl_log_destroy(&vk->pllog);
    free(p);
    ctx->priv = NULL;
}

static bool init(struct ra_ctx *ctx)
{
    if (!g_layer)
        return false;
    struct priv *p = ctx->priv = calloc(1, sizeof(struct priv));
    struct mpvk_ctx *vk = &p->vk;

    vk->pllog = pl_log_create_349(349, &(struct pl_log_params){
        .log_cb = log_cb,
        .log_level = PL_LOG_WARN,
    });
    const char *exts[] = {"VK_KHR_surface", "VK_EXT_metal_surface"};
    vk->vkinst = pl_vk_inst_create(vk->pllog, &(struct pl_vk_inst_params){
        .debug = ctx->opts.debug,
        .extensions = exts,
        .num_extensions = 2,
    });
    if (!vk->vkinst)
        goto error;

    PFN_vkCreateMetalSurfaceEXT create_surface =
        (PFN_vkCreateMetalSurfaceEXT)vk->vkinst->get_proc_addr(vk->vkinst->instance,
                                                               "vkCreateMetalSurfaceEXT");
    VkMetalSurfaceCreateInfoEXT info = {
        .sType = VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT,
        .pLayer = (__bridge const void *)g_layer,
    };
    if (!create_surface || create_surface(vk->vkinst->instance, &info, NULL, &vk->surface) != 0)
        goto error;

    struct ra_ctx_params params = {.swap_buffers = swap_buffers};
    if (!ra_vk_ctx_init(ctx, vk, params, VK_PRESENT_MODE_FIFO_KHR))
        goto error;
    return true;

error:
    uninit(ctx);
    return false;
}

static bool reconfig(struct ra_ctx *ctx)
{
    return do_resize(ctx);
}

static int control(struct ra_ctx *ctx, int *events, int request, void *arg)
{
    (void)arg;
    struct priv *p = ctx->priv;
    // any request may follow a window resize, not only the periodic event check
    if (p->w != atomic_load(&g_pixel_w) || p->h != atomic_load(&g_pixel_h)) {
        do_resize(ctx);
        *events |= VO_EVENT_RESIZE;
    }
    return request == VOCTRL_CHECK_EVENTS ? true : VO_NOTIMPL;
}

__attribute__((visibility("default"), used))
const struct ra_ctx_fns ra_ctx_vulkan_soia = {
    .type = "vulkan",
    .name = "magnoliavk",
    .description = "Magnolia embedded mac/Vulkan (via Metal)",
    .reconfig = reconfig,
    .control = control,
    .init = init,
    .uninit = uninit,
};

void magnolia_vk_sync(void *view);

// create the video layer as the bottom sublayer of `view` (the window's content view).
// must run on the main thread.
void magnolia_vk_attach(void *view)
{
    NSView *v = (__bridge NSView *)view;
    v.wantsLayer = YES;
    CAMetalLayer *layer = [MagnoliaMetalLayer layer];
    layer.opaque = YES;
    layer.backgroundColor = NSColor.blackColor.CGColor;
    layer.framebufferOnly = YES;
    layer.autoresizingMask = kCALayerWidthSizable | kCALayerHeightSizable;
    layer.needsDisplayOnBoundsChange = NO;
    [v.layer insertSublayer:layer atIndex:0];
    g_layer = layer;
    magnolia_vk_sync(view);
}

// match the layer to the view's bounds and backing scale. must run on the main thread.
void magnolia_vk_sync(void *view)
{
    if (!g_layer)
        return;
    NSView *v = (__bridge NSView *)view;
    CGFloat scale = v.window ? v.window.backingScaleFactor : 2.0;
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    g_layer.frame = v.layer.bounds;
    g_layer.contentsScale = scale;
    [CATransaction commit];
    int w = (int)(v.bounds.size.width * scale);
    int h = (int)(v.bounds.size.height * scale);
    atomic_store(&g_pixel_w, w > 0 ? w : 1);
    atomic_store(&g_pixel_h, h > 0 ? h : 1);
}

void magnolia_vk_detach(void)
{
    [g_layer removeFromSuperlayer];
    g_layer = nil;
}
