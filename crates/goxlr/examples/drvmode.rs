//! Sonde (Windows) : mode de flux du pilote Thesycon de la GoXLR, celui de son panneau « Streaming »
//! (« On when Needed » ou « Always On »).
//!
//!     drvmode get
//!     drvmode set <mode> [parametre]
//!
//! Signatures relevees dans la DLL 5.57 : GetDeviceStreamingMode(handle, *u32) et
//! SetDeviceStreamingMode(handle, mode, parametre), qui envoient un bloc de 8 octets au pilote.
//! Arreter le moteur avant de lancer cette sonde : son enumeration invalide les handles du pilote.

#[cfg(not(windows))]
fn main() {
    eprintln!("drvmode : Windows seulement");
}

#[cfg(windows)]
fn main() {
    use libloading::Library;
    type FnVoid = unsafe extern "system" fn() -> u32;
    type FnOpen = unsafe extern "system" fn(u32, *mut u32) -> u32;
    type FnClose = unsafe extern "system" fn(u32) -> u32;
    type FnGet = unsafe extern "system" fn(u32, *mut u32) -> u32;
    type FnSet = unsafe extern "system" fn(u32, u32, u32) -> u32;

    let port = std::env::var("RUSTUDIO_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(14565u16);
    // Meme verrou que le moteur : refus si le service tourne, avant tout acces au pilote.
    let _guard = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("Port {port} indisponible ({e}). Arrete le service Rustudio avant la sonde ; aucun acces au pilote.");
        std::process::exit(2);
    });
    let args: Vec<String> = std::env::args().collect();
    let path = rustudio_goxlr::dll::default_path();
    // SAFETY : DLL du pilote officiel, depuis son dossier d'installation ; signatures relevees dans la DLL 5.57.
    unsafe {
        let lib = Library::new(&path).expect("DLL du pilote introuvable");
        let enumerate = lib.get::<FnVoid>(b"TUSBAUDIO_EnumerateDevices").unwrap();
        let count = lib.get::<FnVoid>(b"TUSBAUDIO_GetDeviceCount").unwrap();
        let open = lib.get::<FnOpen>(b"TUSBAUDIO_OpenDeviceByIndex").unwrap();
        let close = lib.get::<FnClose>(b"TUSBAUDIO_CloseDevice").unwrap();
        let get = lib.get::<FnGet>(b"TUSBAUDIO_GetDeviceStreamingMode").unwrap();
        let set = lib.get::<FnSet>(b"TUSBAUDIO_SetDeviceStreamingMode").unwrap();
        let e = enumerate();
        let n = count();
        let mut h = 0u32;
        let o = open(0, &mut h);
        println!("enumerate=0x{e:x} appareils={n} open=0x{o:x}");
        if o != 0 {
            std::process::exit(1);
        }
        let mut mode = 0xFFFF_FFFFu32;
        let g = get(h, &mut mode);
        println!("avant get=0x{g:x} mode={mode}");
        if args.get(1).map(String::as_str) == Some("set") {
            let m: u32 = args.get(2).and_then(|s| s.parse().ok()).expect("mode attendu");
            let p: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let s = set(h, m, p);
            let mut after = 0xFFFF_FFFFu32;
            let g2 = get(h, &mut after);
            println!("set({m},{p})=0x{s:x} apres get=0x{g2:x} mode={after}");
        }
        close(h);
    }
}
