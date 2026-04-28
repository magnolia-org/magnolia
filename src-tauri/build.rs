use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn directory_contains_file<F>(dir: &Path, matcher: F) -> bool
where
    F: Fn(&str) -> bool,
{
    fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                    && entry.file_name().to_str().map(&matcher).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn has_linux_soia_utils_runtime(dir: &Path) -> bool {
    directory_contains_file(dir, |name| name.starts_with("libsoia_utils.so"))
}

fn has_windows_soia_utils_runtime(dir: &Path) -> bool {
    directory_contains_file(dir, |name| {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".dll") && lower.contains("soia_utils")
    })
}

fn normalize_windows_import_library(
    dir: &Path,
    candidates: &[&str],
    canonical_name: &str,
) -> Option<String> {
    let canonical_path = dir.join(canonical_name);
    if canonical_path.exists() {
        return Some(canonical_name.trim_end_matches(".lib").to_string());
    }

    let existing = candidates.iter().find_map(|candidate| {
        let candidate_path = dir.join(candidate);
        if candidate_path.exists() {
            Some(candidate_path)
        } else {
            None
        }
    })?;

    if let Err(err) = fs::copy(&existing, &canonical_path) {
        panic!(
            "\n[!] Failed to normalize import library to '{}': {}\n    Source: {}\n    Destination: {}\n",
            canonical_name,
            err,
            existing.display(),
            canonical_path.display()
        );
    }

    Some(canonical_name.trim_end_matches(".lib").to_string())
}

fn soia_windows_link_name(dir: &Path) -> Option<String> {
    normalize_windows_import_library(
        dir,
        &[
            "soia_utils.lib",
            "soia_utils.dll.a",
            "libsoia_utils.lib",
            "libsoia_utils.dll.a",
        ],
        "soia_utils.lib",
    )
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mpv_lib_dir = PathBuf::from(&manifest_dir).join("libs").join("mpv");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_triple = env::var("TARGET").unwrap_or_default();

    let windows_link_name = if mpv_lib_dir.join("mpv.lib").exists() {
        Some("mpv")
    } else if mpv_lib_dir.join("mpv-2.lib").exists() {
        Some("mpv-2")
    } else if mpv_lib_dir.join("libmpv.lib").exists() {
        Some("libmpv")
    } else if mpv_lib_dir.join("libmpv-2.lib").exists() {
        Some("libmpv-2")
    } else if mpv_lib_dir.join("libmpv.dll.a").exists() || mpv_lib_dir.join("mpv.dll.a").exists() {
        Some("mpv")
    } else if mpv_lib_dir.join("libmpv-2.dll.a").exists()
        || mpv_lib_dir.join("mpv-2.dll.a").exists()
    {
        Some("mpv-2")
    } else {
        None
    };

    let has_runtime = match target_os.as_str() {
        "macos" => {
            mpv_lib_dir.join("libmpv.dylib").exists() || mpv_lib_dir.join("libmpv.2.dylib").exists()
        }
        "windows" => windows_link_name.is_some(),
        "linux" => {
            mpv_lib_dir.join("libmpv.so").exists()
                || mpv_lib_dir.join("libmpv.so.2").exists()
                || mpv_lib_dir.join("libmpv.so.1").exists()
        }
        _ => mpv_lib_dir.join("libmpv.dylib").exists(),
    };

    if !has_runtime {
        panic!(
            "\n[!] Cannot find libmpv runtime/import library for target '{}'. Please run: npm run setup:libs\n",
            target_triple
        );
    }

    println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
    if target_os == "windows" {
        let link_name = windows_link_name.unwrap_or("mpv");
        println!("cargo:rustc-link-lib={}", link_name);
    } else {
        println!("cargo:rustc-link-lib=mpv");
    }

    match target_os.as_str() {
        "macos" => {
            let soia_utils_dylib = mpv_lib_dir.join("libsoia_utils.dylib");
            if !soia_utils_dylib.exists() {
                panic!(
                    "\n[!] Cannot find libsoia_utils.dylib for target '{}'.\n    Expected in libs/mpv: {}\n    Please run: npm run setup:libs\n",
                    target_triple,
                    soia_utils_dylib.display()
                );
            }
            println!("cargo:rustc-link-lib=dylib=soia_utils");
        }
        "linux" => {
            if !has_linux_soia_utils_runtime(&mpv_lib_dir) {
                panic!(
                    "\n[!] Cannot find libsoia_utils.so* for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: npm run setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }
            println!("cargo:rustc-link-lib=dylib=soia_utils");
        }
        "windows" => {
            if !has_windows_soia_utils_runtime(&mpv_lib_dir) {
                panic!(
                    "\n[!] Cannot find soia_utils.dll for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: npm run setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }

            let Some(link_name) = soia_windows_link_name(&mpv_lib_dir) else {
                panic!(
                    "\n[!] Cannot find soia_utils import library (.lib/.dll.a) for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: npm run setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            };

            println!("cargo:rustc-link-lib={}", link_name);
        }
        _ => {}
    }

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=IOKit");
    }

    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_macos.sh");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_linux.sh");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_windows.mjs");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs.mjs");
    println!("cargo:rerun-if-changed=libs/mpv");
    println!("cargo:rerun-if-env-changed=TARGET");

    tauri_build::build();
}
