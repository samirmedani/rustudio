fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // l'archive de l'appli, fabriquee par la release (scripts/build-release.ps1) ; vide en developpement
    println!("cargo:rerun-if-env-changed=RUSTUDIO_PAYLOAD");
    match std::env::var("RUSTUDIO_PAYLOAD") {
        Ok(p) if !p.is_empty() => {
            println!("cargo:rerun-if-changed={p}");
            std::fs::copy(&p, out.join("payload.zip")).expect("archive RUSTUDIO_PAYLOAD");
        }
        _ => std::fs::write(out.join("payload.zip"), b"").unwrap(),
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("setup.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
