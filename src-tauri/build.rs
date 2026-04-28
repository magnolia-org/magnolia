fn main() {
    tauri_build::build();

    // macOS: use bundled libmpv + libsoia_utils from src-tauri/libs/mpv/lib/
    // (downloaded from https://github.com/FengZeng/mpv/releases — includes Metal support).
    #[cfg(target_os = "macos")]
    {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let libs_dir = manifest_dir.join("libs/mpv/lib");
        assert!(
            libs_dir.exists(),
            "[!] {} not found. Download the macOS arm64 bundle from \
             https://github.com/FengZeng/mpv/releases and extract it to src-tauri/libs/mpv/",
            libs_dir.display()
        );
        println!("cargo:rustc-link-search=native={}", libs_dir.display());
        println!("cargo:rustc-link-lib=dylib=mpv");
        println!("cargo:rustc-link-lib=dylib=soia_utils");
        // Embed the rpath so the binary finds the dylibs at runtime without
        // requiring DYLD_LIBRARY_PATH to be set.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libs_dir.display());
        println!("cargo:rerun-if-changed=libs/mpv/lib/libmpv.dylib");
        println!("cargo:rerun-if-changed=libs/mpv/lib/libsoia_utils.dylib");
    }

    // Non-macOS: link system/Homebrew libmpv.
    #[cfg(not(target_os = "macos"))]
    {
        println!("cargo:rerun-if-env-changed=LIBMPV_LIB_DIR");
        if let Ok(lib_dir) = std::env::var("LIBMPV_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", lib_dir);
        }
        println!("cargo:rustc-link-lib=dylib=mpv");
    }

    // macOS: Metal + QuartzCore frameworks needed by libmpv/MoltenVK.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=IOKit");
    }
}
