use rustudio_cooler_master::{key_frames, sim::Sim, Changes, Keyboard, Transport};
use serde_json::json;

#[test]
fn lecture_complete_du_clavier() {
    let s = Keyboard::new(Sim::default()).read().unwrap();
    assert!(s.wired);
    assert_eq!(s.firmware.as_deref(), Some("1.07"));
    assert_eq!(s.active_effect, 0);
    assert_eq!(s.effects.len(), 20);
    assert_eq!(s.effects[0].colour, "#8A2BFF");
    assert_eq!((s.effects[1].speed, s.effects[1].reverse, s.effects[1].brightness), (2, false, 255));
    assert_eq!(s.keys.len(), 102, "le clavier ne relit que 102 LED");
    assert_eq!(s.keys[19], "#00FF00");
}

#[test]
fn effet_vitesse_sens_luminosite_et_couleur() {
    let mut sim = Sim::default();
    let after = Keyboard::new(&mut sim).apply(&Changes { active_effect: Some(1), speed: Some(4), reverse: Some(true), brightness: Some(0x3F), ..Default::default() }).unwrap();
    assert_eq!(after.active_effect, 1);
    assert_eq!((after.effects[1].speed, after.effects[1].reverse, after.effects[1].brightness), (4, true, 0x3F));
    // octet 2 : vitesse dans les bits bas, sens dans le bit 0x10 ; les octets inconnus restent tels quels
    assert_eq!(&sim.memory[10..20], &[0x3F, 0xFF, 0x14, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06]);
    let after = Keyboard::new(&mut sim).apply_json(&json!({"effect": 0, "colour": "#00ff7f"})).unwrap();
    assert_eq!(after.active_effect, 1, "régler une fiche ne change pas l'effet affiché");
    assert_eq!(after.effects[0].colour, "#00FF7F");
    assert_eq!(sim.memory[200], 1);
}

#[test]
fn couleurs_des_touches_en_trame_continue() {
    let frames = key_frames(&[[1, 2, 3]; 104]);
    assert_eq!(frames.len(), 7);
    assert_eq!(&frames[0][..6], &[0x0A, 0x87, 0, 0, 0xA0, 0x01]);
    assert_eq!(&frames[0][6..10], &[1, 2, 3, 0xFF]);
    assert_eq!(frames[6][1], 6);
    let mut sim = Sim::default();
    let colours: Vec<String> = (0..104).map(|i| format!("#{:02X}{:02X}00", i, 255 - i)).collect();
    let after = Keyboard::new(&mut sim).apply(&Changes { keys: Some(colours.clone()), ..Default::default() }).unwrap();
    assert_eq!(after.active_effect, 4, "les couleurs par touche ne s'affichent qu'avec l'effet Personnalisé");
    assert_eq!(after.keys, colours[..102]);
    assert_eq!(sim.keys[103], [103, 152, 0]);
}

#[test]
fn trame_incomplete_ignoree_et_ecriture_non_confirmee() {
    let mut sim = Sim::default();
    let before = sim.keys;
    for frame in key_frames(&[[9, 9, 9]; 104]).iter().take(5) {
        sim.send(frame).unwrap();
    }
    assert_eq!(sim.keys, before, "comme le vrai clavier, une trame incomplète ne change rien");
}

#[test]
fn rapports_parasites_ignores() {
    let mut sim = Sim::default();
    sim.noise = true;
    let s = Keyboard::new(&mut sim).read().unwrap();
    assert_eq!(s.effects[15].colour, "#007FFF");
    assert_eq!(s.keys.len(), 102);
}

#[test]
fn demandes_refusees_avant_toute_ecriture() {
    let mut sim = Sim::default();
    for bad in [json!({"speed": 5}), json!({"active_effect": 17}), json!({"colour": "rouge"}), json!({"keys": []}),
                json!({"dpi": 800}), json!({}), json!({"effect": 25, "brightness": 10})] {
        assert!(Keyboard::new(&mut sim).apply_json(&bad).is_err(), "{bad}");
    }
    assert_eq!(sim.writes, 0);
}

#[test]
fn clavier_debranche() {
    let mut sim = Sim::default();
    sim.disconnected = true;
    assert!(Keyboard::new(sim).read().is_err());
}
