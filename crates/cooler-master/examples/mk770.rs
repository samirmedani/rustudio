//! Vérification sur un vrai MK770 branché par câble, sans installation (Windows seulement).
//!   mk770            lecture seule : effet actif, fiches, couleurs des touches
//!   mk770 --verify   écrit puis restaure (luminosité, effet, couleurs des touches), chaque étape relue
//! Ne pas lancer pendant que Rustudio pilote le clavier.
#[cfg(windows)]
fn main() -> Result<(), String> {
    use rustudio_cooler_master::{Changes, Hid, Keyboard};
    let mut kb = Keyboard::new(Hid::open()?);
    let start = std::time::Instant::now();
    let before = kb.read()?;
    println!("lecture complète en {} ms", start.elapsed().as_millis());
    println!("firmware {:?}, effet actif {} ({})", before.firmware, before.active_effect, rustudio_core::mk770::effect_name(before.active_effect));
    for e in &before.effects {
        println!("  effet {:2} luminosité {:3} vitesse {} sens {} couleur {}", e.id, e.brightness, e.speed, if e.reverse { "inverse" } else { "normal" }, e.colour);
    }
    println!("touches relues : {}, allumées : {}", before.keys.len(), before.keys.iter().filter(|k| *k != "#000000").count());
    if !std::env::args().any(|a| a == "--verify") {
        return Ok(());
    }
    let active = before.active_effect;
    let effect = &before.effects[active as usize];
    let result = (|| -> Result<(), String> {
        let dim = if effect.brightness == 0xBF { 0x7F } else { 0xBF };
        let s = kb.apply(&Changes { brightness: Some(dim), ..Default::default() })?;
        println!("OK luminosité {} relue", s.effects[active as usize].brightness);
        let other = if active == 1 { 0 } else { 1 };
        let s = kb.apply(&Changes { active_effect: Some(other), ..Default::default() })?;
        println!("OK effet {} relu", s.active_effect);
        let s = kb.apply(&Changes { active_effect: Some(active.min(16)), ..Default::default() })?;
        println!("OK retour à l'effet {}", s.active_effect);
        // mêmes couleurs, effet actif conservé : l'écriture et sa relecture complète, sans rien changer à l'écran
        let s = kb.apply(&Changes { active_effect: Some(active.min(16)), keys: Some(before.keys.clone()), ..Default::default() })?;
        println!("OK couleurs des touches écrites et relues ({})", s.keys.len());
        Ok(())
    })();
    // restauration, même après un échec
    let restore = kb.apply(&Changes { effect: Some(active), brightness: Some(effect.brightness), active_effect: (active <= 16).then_some(active), ..Default::default() });
    let after = kb.read()?;
    let same = after.active_effect == before.active_effect && after.effects == before.effects && after.keys == before.keys;
    println!("restauration : {}", if same { "identique à l'état de départ" } else { "DIFFÉRENTE" });
    result?;
    restore?;
    if !same {
        return Err("état final différent de l'état de départ".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("La sonde MK770 fonctionne sous Windows.");
}
