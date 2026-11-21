//! Sonde en lecture seule du moniteur WASAPI, sans service ni modification des périphériques par défaut.
use rustudio_windows_desktop::{
    audio,
    spectrum::{FLOOR, Monitor, Source},
};
use std::time::{Duration, Instant};
fn main() -> Result<(), String> {
    let inputs = std::env::args().any(|a| a == "--input");
    let inventory = audio::inventory();
    let rows = inventory[if inputs { "inputs" } else { "outputs" }]
        .as_array()
        .ok_or("Inventaire audio indisponible")?;
    let row = rows
        .iter()
        .find(|r| r["default"] == true)
        .or_else(|| rows.first())
        .ok_or("Aucune source audio")?;
    let id = row["id"].as_str().ok_or("Identifiant audio absent")?;
    let monitor = Monitor::new(false);
    let source = Source {
        id: id.into(),
        output: !inputs,
    };
    monitor.configure(vec![source.clone()], Some(source));
    let until = Instant::now() + Duration::from_secs(3);
    let (mut frames, mut rate, mut peak, mut active) = (0, 0, FLOOR, 0);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(60));
        let f = monitor.frame();
        if let Some(e) = f.error {
            return Err(e);
        }
        if f.ready {
            frames += 1;
            rate = f.rate;
            peak = peak.max(f.peak_db);
            if f.bands.iter().any(|v| *v > FLOOR + 10.) {
                active += 1;
            }
        }
    }
    monitor.stop();
    let until = Instant::now() + Duration::from_secs(2);
    while monitor.frame().ready {
        if Instant::now() > until {
            return Err("Le moniteur ne s’est pas arrêté".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if frames < 10 || rate == 0 {
        return Err("Pas de données du moniteur".into());
    }
    println!(
        "OK : {} · {frames} lectures · {rate} Hz · pic {peak:.1} dB · {active} spectres actifs · arrêt confirmé",
        if inputs {
            "entrée Windows"
        } else {
            "sortie Windows"
        }
    );
    Ok(())
}
