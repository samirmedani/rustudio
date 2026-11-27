//! Verification silencieuse sur une vraie Logitech C920 branchee en USB (Windows seulement).
//! N'ouvre jamais le flux video ni le micro. En mode --verify, modifie temporairement la luminosite
//! d'un cran, relit pour confirmer, puis restaure la valeur exacte d'origine.

#[cfg(windows)]
fn main() -> Result<(), String> {
    use rustudio_logitech_c920::win::KsCamera;
    use rustudio_logitech_c920::{Camera, ControlPatch};

    let verify = std::env::args().any(|a| a == "--verify");
    let cam_dev = KsCamera::open()?;
    let mut cam = Camera::new(cam_dev);
    let initial = cam.read()?;
    println!("{}", serde_json::to_string_pretty(&initial).map_err(|e| e.to_string())?);

    if verify {
        let r = initial.ranges.brightness;
        let probe = if initial.brightness + r.step <= r.max {
            initial.brightness + r.step
        } else {
            initial.brightness - r.step
        };
        let changed = cam.apply_patch(
            &ControlPatch {
                brightness: Some(probe),
                ..Default::default()
            },
            true,
        )?;
        let restore_result = cam.apply_patch(
            &ControlPatch {
                brightness: Some(initial.brightness),
                ..Default::default()
            },
            true,
        );
        if changed.brightness != probe {
            let _ = restore_result;
            return Err(format!("Ecriture luminosite non confirmee : attendu {probe}, relu {}", changed.brightness));
        }
        let restored = restore_result?;
        if restored.brightness != initial.brightness {
            return Err(format!("Restauration luminosite echouee : attendu {}, relu {}", initial.brightness, restored.brightness));
        }
        eprintln!(
            "OK : lecture, ecriture luminosite ({} -> {} -> {}) et restauration confirmees sans audio ni flux video.",
            initial.brightness, probe, restored.brightness
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("La sonde Logitech C920 fonctionne sous Windows.");
}
