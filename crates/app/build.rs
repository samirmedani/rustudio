fn main() {
    // icône et manifeste Windows (DPI par écran, contrôles communs v6)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=icons/icon.ico");
        println!("cargo:rerun-if-changed=app.manifest");
        embed_resource::compile("app.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
