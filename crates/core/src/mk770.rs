//! Cooler Master MK770 : données sans matériel, communes au protocole, au moteur et à l'appli.
//! Noms des effets relevés effet par effet sur un vrai MK770 (docs/MK770.md). Disposition des LED d'après
//! led_map.json de CoolerHacker-MK770 (Defacedz, licence MIT, voir crates/cooler-master).
use serde::Serialize;

/// Un effet intégré du clavier, tel que Rustudio le propose.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct EffectInfo {
    pub id: u8,
    pub name: &'static str,
    /// la couleur de la fiche (octets 3 à 5) compte pour cet effet
    pub colour: bool,
    /// la vitesse (octet 2, bits 0 à 3) compte pour cet effet
    pub speed: bool,
    /// le sens (octet 2, bit 0x10), vérifié sur la vague arc-en-ciel
    pub direction: bool,
    /// ne s'allume qu'en tapant
    pub reactive: bool,
}

const fn fx(id: u8, name: &'static str, colour: bool, speed: bool, direction: bool, reactive: bool) -> EffectInfo {
    EffectInfo { id, name, colour, speed, direction, reactive }
}

/// Effets 0 à 16, dans l'ordre du firmware. 17 à 19 n'affichent rien sur le MK770 : ils ne sont pas proposés.
pub const EFFECTS: [EffectInfo; 17] = [
    fx(0, "Couleur fixe", true, false, false, false),
    fx(1, "Vague arc-en-ciel", false, true, true, false),
    fx(2, "Croix", true, true, false, true),
    fx(3, "Fondu réactif", true, true, false, true),
    fx(4, "Personnalisé", false, false, false, false),
    fx(5, "Étoiles", true, true, false, false),
    fx(6, "Neige", true, true, false, false),
    fx(7, "Cycle de couleurs", false, true, false, false),
    fx(8, "Respiration", true, true, false, false),
    fx(9, "Ondulation", true, true, false, true),
    fx(10, "Éclat réactif", true, true, false, true),
    fx(11, "Battement de cœur", true, true, false, false),
    fx(12, "Boule de feu", true, true, false, true),
    fx(13, "Spectre circulaire", false, true, false, false),
    fx(14, "Tornade", false, true, false, false),
    fx(15, "Gouttes d'eau", true, true, false, true),
    fx(16, "Éteint", false, false, false, false),
];

/// Effet « Personnalisé » : affiche la couleur de chaque touche.
pub const CUSTOM: u8 = 4;
/// Effet « Éteint ».
pub const OFF: u8 = 16;
/// Effets enregistrés dans le clavier (0 à 19).
pub const RECORDS: u8 = 20;
/// Vitesse : 0 (lente) à 4 (rapide), comme les cinq crans de Fn + F10 / F11.
pub const SPEED_MAX: u8 = 4;
/// Luminosité : les cinq crans de Fn + F8.
pub const BRIGHTNESS_STEPS: [u8; 5] = [0x00, 0x3F, 0x7F, 0xBF, 0xFF];
/// LED écrites par le clavier (relues : 102).
pub const LEDS: usize = 104;
pub const LEDS_READ: usize = 102;

pub fn effect(id: u8) -> Option<&'static EffectInfo> {
    EFFECTS.iter().find(|e| e.id == id)
}

/// Nom affiché d'un effet, y compris 17 à 19 (rien à l'écran).
pub fn effect_name(id: u8) -> &'static str {
    effect(id).map(|e| e.name).unwrap_or("Éteint")
}

/// Une touche dessinée : libellé, largeur (en touches), LED qui l'éclairent (vide : pas de LED à elle).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Key {
    pub label: &'static str,
    pub width: f32,
    pub leds: &'static [u8],
}

const fn k(label: &'static str, leds: &'static [u8]) -> Key {
    Key { label, width: 1.0, leds }
}
const fn w(label: &'static str, width: f32, leds: &'static [u8]) -> Key {
    Key { label, width, leds }
}
/// Espace vide entre deux blocs de touches.
const fn gap(width: f32) -> Key {
    Key { label: "", width, leds: &[] }
}

/// Disposition AZERTY du MK770, rangée par rangée. La flèche droite partage la LED de la flèche bas ;
/// la barre d'espace en porte trois.
pub const LAYOUT: [&[Key]; 6] = [
    &[k("Échap", &[0]), gap(0.5), k("F1", &[1]), k("F2", &[2]), k("F3", &[3]), k("F4", &[4]), gap(0.25), k("F5", &[5]), k("F6", &[6]),
      k("F7", &[7]), k("F8", &[8]), gap(0.25), k("F9", &[9]), k("F10", &[10]), k("F11", &[11]), k("F12", &[12]), gap(0.25),
      k("Suppr", &[13]), k("Inser", &[14]), k("PgPréc", &[15]), k("PgSuiv", &[16])],
    &[k("²", &[18]), k("1", &[19]), k("2", &[20]), k("3", &[21]), k("4", &[22]), k("5", &[23]), k("6", &[24]), k("7", &[25]),
      k("8", &[26]), k("9", &[27]), k("0", &[28]), k(")", &[29]), k("=", &[30]), w("Retour", 2.0, &[31]), gap(0.25),
      k("Verr", &[32]), k("/", &[33]), k("*", &[34]), k("-", &[35])],
    &[w("Tab", 1.5, &[36]), k("A", &[37]), k("Z", &[38]), k("E", &[39]), k("R", &[40]), k("T", &[41]), k("Y", &[42]), k("U", &[43]),
      k("I", &[44]), k("O", &[45]), k("P", &[46]), k("^", &[47]), k("$", &[48]), w("Entrée", 1.5, &[66]), gap(0.25),
      k("7", &[50]), k("8", &[51]), k("9", &[52]), k("+", &[53])],
    &[w("Verr Maj", 1.75, &[54]), k("Q", &[55]), k("S", &[56]), k("D", &[57]), k("F", &[58]), k("G", &[59]), k("H", &[60]),
      k("J", &[61]), k("K", &[62]), k("L", &[63]), k("M", &[64]), k("ù", &[65]), k("*", &[49]), gap(1.5),
      k("4", &[67]), k("5", &[68]), k("6", &[69])],
    &[w("Maj", 1.25, &[70]), k("<", &[71]), k("W", &[72]), k("X", &[73]), k("C", &[74]), k("V", &[75]), k("B", &[76]), k("N", &[77]),
      k(",", &[78]), k(";", &[79]), k(":", &[80]), k("!", &[81]), w("Maj", 1.75, &[82]), k("↑", &[83]),
      k("1", &[84]), k("2", &[85]), k("3", &[86]), k("Entrée", &[87])],
    &[w("Ctrl", 1.25, &[88]), w("Win", 1.25, &[89]), w("Alt", 1.25, &[90]), w("Espace", 6.25, &[91, 92, 93]), k("AltGr", &[94]),
      k("Menu", &[95]), k("Ctrl", &[96]), k("←", &[97]), k("↓", &[98]), k("→", &[98]), k("0", &[101]), k(".", &[100])],
];

/// « #8A2BFF » ou « 8a2bff » → [0x8A, 0x2B, 0xFF].
pub fn parse_colour(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn format_colour(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effets_dans_l_ordre_du_firmware() {
        for (i, e) in EFFECTS.iter().enumerate() {
            assert_eq!(e.id as usize, i);
        }
        assert_eq!(effect(CUSTOM).unwrap().name, "Personnalisé");
        assert_eq!(effect_name(OFF), "Éteint");
        assert_eq!(effect_name(19), "Éteint");
        assert!(effect(1).unwrap().direction);
    }

    #[test]
    fn chaque_led_eclairee_une_fois_au_plus() {
        let mut seen = [0u8; LEDS];
        for row in LAYOUT {
            for key in row {
                for &led in key.leds {
                    seen[led as usize] += 1;
                }
            }
        }
        // la LED 98 éclaire la flèche bas et la flèche droite ; 17, 99, 102 et 103 n'ont pas de touche
        for (led, n) in seen.iter().enumerate() {
            let expected = match led { 98 => 2, 17 | 99 | 102 | 103 => 0, _ => 1 };
            assert_eq!(*n, expected, "LED {led}");
        }
    }

    #[test]
    fn couleurs() {
        assert_eq!(parse_colour("#8a2BFF"), Some([0x8A, 0x2B, 0xFF]));
        assert_eq!(parse_colour("12345"), None);
        assert_eq!(parse_colour("GG0000"), None);
        assert_eq!(format_colour([0x8A, 0x2B, 0xFF]), "#8A2BFF");
    }
}
