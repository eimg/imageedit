fn main() {
    println!("cargo:rerun-if-changed=windows/app.rc");
    println!("cargo:rerun-if-changed=windows/AppIcon.ico");

    // Like GPUI's manifest/shader build, resources are compiled by the native
    // Windows SDK. A macOS cross-check only type-checks the Windows application.
    #[cfg(target_os = "windows")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Require the icon in native builds rather than silently shipping the
        // default Windows icon if the resource compiler is unavailable.
        embed_resource::compile_for("windows/app.rc", ["lime"], embed_resource::NONE)
            .manifest_required()
            .expect("could not embed the Lime Windows icon");
    }
}
