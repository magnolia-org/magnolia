/// Platform-specific embedding of libmpv's render output into the native window.
///
/// All platforms now use soia_utils for embedding. This module is a no-op stub
/// kept for compatibility; soia_utils_create() in handle.rs handles all window setup.

pub struct EmbedHandle;

