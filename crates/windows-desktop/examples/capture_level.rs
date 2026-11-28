//! Client de mesure pour une entrée virtuelle explicite. L'enregistrement reste
//! en mémoire. Seuls les niveaux numériques de la seconde moitié sont affichés.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{atomic::AtomicBool, Arc};
    let guid = std::env::args()
        .nth(1)
        .ok_or("GUID du micro virtuel requis")?;
    let parsed = windows::core::GUID::try_from(guid.trim_matches(['{', '}']))?;
    let id = format!("{{0.0.1.00000000}}.{{{parsed:?}}}");
    let (samples, rate) = rustudio_windows_desktop::soundcheck::record_input(
        &id,
        4.,
        Arc::new(AtomicBool::new(false)),
    )?;
    if samples.len() < rate as usize || samples.iter().any(|x| !x.is_finite()) {
        return Err("Capture incomplète ou non finie".into());
    }
    let tail = &samples[samples.len() / 2..];
    let peak = tail.iter().fold(0f32, |p, x| p.max(x.abs()));
    let rms = (tail.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
    println!(
        "{}",
        serde_json::json!({"frames": samples.len(), "rate": rate, "peak": peak, "rms": rms})
    );
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("La mesure utilise une entrée virtuelle Windows.");
}
