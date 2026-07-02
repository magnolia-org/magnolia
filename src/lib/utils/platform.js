// Platform detection via the Tauri OS plugin. UA sniffing is unreliable in
// this app: both macOS WKWebView and Linux WebKitGTK are WebKit, and WebKitGTK
// can report a Mac-like user agent, so feature/UA checks meant for macOS also
// pass on Linux. plugin-os reads the real OS from the backend and is
// synchronous in Tauri v2.
import { platform } from "@tauri-apps/plugin-os";

const current = platform();

export const isMacOS = current === "macos";
export const isWindows = current === "windows";
export const isLinux = current === "linux";
