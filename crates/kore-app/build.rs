fn main() {
    // Embed the app icon + version info into kore.exe (Explorer/taskbar icon).
    // Check the *target* OS: `cfg(windows)` in a build script is the host, which
    // would skip this when cross-compiling (cargo xwin) and run it on Windows
    // hosts building for other targets.
    println!("cargo:rerun-if-changed=packaging/windows/kore.rc");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // llvm-rc/rc.exe resolve relative paths against the preprocessed copy in
        // OUT_DIR, so hand the icon over as an absolute path.
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap().replace('\\', "/");
        let icon = format!("KORE_ICON=\"{dir}/assets/icons/icon.ico\"");
        embed_resource::compile("packaging/windows/kore.rc", embed_resource::ParamsMacros([icon.as_str()]))
            .manifest_optional()
            .expect("failed to compile Windows resources");
    }
}
