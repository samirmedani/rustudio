//! Test système pour une VM sans sortie physique. Le périphérique de capture est toujours explicite.
//! L’environnement de test place la DLL du paquet dans Program Files avant de lancer cet exemple sous SYSTEM.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rustudio_windows_mic::{control::Controls, mapping::Mapping, registry, Voice};
    let mut args = std::env::args().skip(1);
    let guid = args.next().ok_or("GUID du micro virtuel requis")?;
    let mode = args.next().ok_or("action de test requise")?;
    if !rustudio_core::micns::is_guid(&guid) {
        return Err("GUID invalide".into());
    }
    match mode.as_str() {
        "attach" => {
            registry::attach(&guid, false)?;
            println!("Attach OK");
        }
        "remove" => {
            registry::detach(&guid)?;
            println!("Remove OK");
        }
        "verify" => {
            let (frames, callbacks) = rustudio_windows_mic::capture::verify(&guid)?;
            println!("WASAPI frames={frames} Rustudio APO callbacks={callbacks}");
        }
        "baseline" => {
            let _memory = Mapping::open(&guid, true)?;
            let (frames, callbacks) = rustudio_windows_mic::capture::measure(&guid)?;
            if frames == 0 || callbacks != 0 {
                return Err(format!(
                    "Capture brute inattendue : {frames} trames, {callbacks} rappels"
                )
                .into());
            }
            println!("Capture brute WASAPI frames={frames} callbacks={callbacks}");
        }
        "fault" => {
            let map = Mapping::open(&guid, false)?;
            map.shared()
                .faults
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        "control" | "probe" => {
            let map = Mapping::open(&guid, true)?;
            let mut v = Voice {
                enabled: true,
                ..Voice::default()
            };
            v.dynamics.limiter = true;
            map.shared().publish(Controls::new(&v, false, 90, 200));
            if mode == "control" {
                let marker = std::path::PathBuf::from(
                    args.next()
                        .ok_or("Fichier de synchronisation de la VM requis")?,
                );
                std::fs::write(&marker, b"ready")?;
                for _ in 0..1200 {
                    if !marker.exists() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                return Ok(());
            }
            let (frames, n) = rustudio_windows_mic::capture::verify(&guid)?;
            println!("WASAPI frames={frames} Rustudio APO callbacks={n}");
        }
        _ => return Err("Action inconnue".into()),
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Utiliser la VM de test Windows.");
}
