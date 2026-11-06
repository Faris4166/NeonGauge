//! Embeds the app icon into the Windows executable (Explorer, shortcuts, MSI).

fn main() {
    println!("cargo:rerun-if-changed=assets/neongauge.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/neongauge.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to embed the Windows icon resource");
    }
}
