fn main() {
    // icône, manifeste Windows (DPI par écran, contrôles communs v6) et version du fichier
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=icons/icon.ico");
        println!("cargo:rerun-if-changed=app.manifest");
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
        let rc_path = |name: &str| dir.join(name).display().to_string().replace('\\', "\\\\");
        let version = env!("CARGO_PKG_VERSION");
        let numbers: Vec<u16> = version.split(['.', '-']).take(3).map(|n| n.parse().unwrap_or(0)).collect();
        let (major, minor, patch) = (numbers[0], numbers.get(1).copied().unwrap_or(0), numbers.get(2).copied().unwrap_or(0));
        // Propriétés du fichier dans l'Explorateur : version, nom du produit et éditeur.
        let rc = format!(
            "#pragma code_page(65001)\n1 ICON \"{icon}\"\n1 24 \"{manifest}\"\n\
             1 VERSIONINFO\nFILEVERSION {major},{minor},{patch},0\nPRODUCTVERSION {major},{minor},{patch},0\n\
             FILEOS 0x40004\nFILETYPE 0x1\nBEGIN\n  BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"040C04B0\"\n    BEGIN\n\
             VALUE \"CompanyName\", \"Rustudio\"\n      VALUE \"FileDescription\", \"Rustudio\"\n\
             VALUE \"FileVersion\", \"{version}\"\n      VALUE \"ProductName\", \"Rustudio\"\n\
             VALUE \"ProductVersion\", \"{version}\"\n      VALUE \"OriginalFilename\", \"Rustudio.exe\"\n    END\n  END\n\
             BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x40C, 1200\n  END\nEND\n",
            icon = rc_path("icons/icon.ico"),
            manifest = rc_path("app.manifest"),
        );
        std::fs::write(&out, rc).unwrap();
        embed_resource::compile(&out, embed_resource::NONE).manifest_required().unwrap();
    }
}
