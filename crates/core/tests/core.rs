//! Tests du coeur (aucun appareil).
use rustudio_core::dsp::{self, fit_dsp, gate_attenuation, Fx, RESIDUAL_NOISE_DB};
use rustudio_core::hpeq::{config_text, hp_filters, normalize_hp_eq, style};
use rustudio_core::release::{parse_release, Names};
use rustudio_core::state::{self, normalize_cal, normalize_state, MicCal};
use rustudio_core::{lighting, routing, semver};
use serde_json::json;

fn cal() -> MicCal {
    normalize_cal(&json!({"type": "condenser", "gain_db": 30, "t": 1.0, "noise_db": -63.0, "noise_peak_db": -60.0,
        "voice_low_db": -36.0, "voice_db": -24.0, "voice_peak_db": -12.0})).unwrap()
}

#[test]
fn semver_et_ordre() {
    assert!(semver::lt("4.8.0", "5.0.0"));
    assert!(semver::lt("v1.2.3-beta.1", "1.2.3"));
    assert!(semver::lt("1.2.3-beta.2", "1.2.3-beta.10"));
    assert!(!semver::lt("1.2.3", "1.2.3"));
    assert!(semver::parse("01.2.3").is_none());
    assert!(semver::parse("1.2").is_none());
    assert!(!semver::lt("n'importe", "1.0.0"));
}

#[test]
fn release_lue_ou_refusee() {
    let n = Names { repo: "x/y", asset: "Rustudio.zip", hash_asset: "Rustudio.sha256", download_prefix: "https://github.com/x/y/releases/download/" };
    let ok = json!({"tag_name": "v5.0.0", "draft": false, "prerelease": false, "body": "notes", "html_url": "https://github.com/x/y/releases/tag/v5.0.0",
        "assets": [{"name": "Rustudio.zip", "size": 12, "browser_download_url": "https://github.com/x/y/releases/download/v5.0.0/Rustudio.zip",
                    "digest": format!("sha256:{}", "a".repeat(64))}]});
    let r = parse_release(&ok, &n).unwrap();
    assert_eq!(r.version, "5.0.0");
    assert_eq!(r.asset.as_ref().unwrap().sha256.as_deref(), Some("a".repeat(64).as_str()));
    let mut bad = ok.clone();
    bad["prerelease"] = json!(true);
    assert!(parse_release(&bad, &n).is_none());
    let mut other = ok.clone();
    other["assets"][0]["browser_download_url"] = json!("https://evil.example/Rustudio.zip");
    assert!(parse_release(&other, &n).unwrap().asset.is_none(), "archive hors des releases du projet ignoree");
}

#[test]
fn gate_entre_bruit_et_voix() {
    let d = fit_dsp("studio", Some(&cal()), Some(30));
    assert!((-54..=-40).contains(&d.gate.threshold), "{:?}", d.gate);
    assert!(d.gate.threshold <= -36 - 10 + 1, "bien sous les passages calmes");
    assert_eq!(d.comp.threshold, -28);
    assert!((4..=10).contains(&d.comp.makeup), "{:?}", d.comp);
    let att = -gate_attenuation(d.gate.attenuation) as f64;
    assert!(att <= dsp::GATE_SOFT_DB, "jamais de coupure seche : {att}");
    assert!(-60.0 + d.comp.makeup as f64 - att >= -75.0 - 1.0 || att >= dsp::GATE_SOFT_DB - 1.0);
    let _ = RESIDUAL_NOISE_DB;
}

#[test]
fn piece_bruyante_coupe_plus() {
    let mut loud = cal();
    loud.noise_db = -48.0;
    loud.noise_peak_db = -44.0;
    assert!(fit_dsp("studio", Some(&loud), Some(30)).gate.attenuation >= fit_dsp("studio", Some(&cal()), Some(30)).gate.attenuation);
}

#[test]
fn sans_mesure_valeurs_d_usine() {
    assert_eq!(fit_dsp("studio", None, Some(30)), dsp::preset_dsp("studio"));
    assert_eq!(fit_dsp("brut", Some(&cal()), Some(30)), dsp::preset_dsp("brut"));
}

#[test]
fn mesure_relue_ou_rejetee() {
    let c = cal();
    assert_eq!(normalize_cal(&serde_json::to_value(&c).unwrap()), Some(c));
    assert!(normalize_cal(&json!({"type": "condenser"})).is_none());
    assert!(normalize_cal(&json!({"type": "ruban", "gain_db": 30, "noise_db": -1, "noise_peak_db": -1, "voice_low_db": -1, "voice_db": -1, "voice_peak_db": -1})).is_none());
}

#[test]
fn voix_naturelle_par_defaut() {
    let st = state::default_state();
    assert_eq!(st.fx, Fx::default());
    assert!(!st.route("Microphone", "Headphones"), "pas de retour micro par defaut");
    let e = dsp::effective_dsp(&st.dsp, &st.fx);
    assert!(!e.gate.enabled);
    assert_eq!((e.comp.ratio, e.comp.makeup, e.deesser), (0, 0, 0));
    assert!(e.eq.values().all(|v| *v == 0));
}

#[test]
fn migration_voix_naturelle() {
    let old = json!({"mic_preset": "studio", "dsp": dsp::preset_dsp("studio"), "mic": {"type": "condenser", "gain_db": 33},
                     "routing": {"Microphone": {"Headphones": true}}});
    let st = normalize_state(&old);
    assert_eq!(st.fx, Fx::default());
    assert!(!st.route("Microphone", "Headphones"));
    assert_eq!(st.mic_preset, "brut");
    let mut kept = old.clone();
    kept["fx"] = json!({"gate": true});
    let st = normalize_state(&kept);
    assert!(st.route("Microphone", "Headphones"), "etat 4.8 : retour micro garde");
    assert!(st.fx.gate);
}

#[test]
fn profil_v3_relu() {
    let st = normalize_state(&json!({"faders": {"A": "Mic"}, "mutes": {"Mic": true}, "user_scenes": {"Soir": {"faders": {"A": "Music"}}}}));
    assert!(st.muted("Mic"));
    assert_eq!(st.mute_modes["A"], "all");
    assert!(st.user_scenes.contains_key("Soir"));
}

#[test]
fn etat_tolerant() {
    let st = normalize_state(&json!({"volumes": {"Mic": "300", "Zzz": 4}, "colours": {"Global": "#12abef", "Accent": "pas une couleur"},
        "faders": {"a": "Music"}, "mic": {"gain_db": 99.6}}));
    assert_eq!(st.volume("Mic"), 255);
    assert_eq!(st.colours["Global"], vec!["12ABEF", "12ABEF"]);
    assert_eq!(st.colours["Accent"], vec!["00FFFF", "FFFFFF"], "couleur invalide ignoree");
    assert_eq!(st.faders["A"], "Music");
    assert_eq!(st.mic.gain_db, 72);
}

#[test]
fn scene_perso_garde_mutes_et_traitements() {
    let mut st = state::default_state();
    st.fx.gate = true;
    st.volumes.insert("Chat".into(), 10);
    let sc = state::user_scene_of(&st);
    let mut now = st.clone();
    now.volumes.insert("Chat".into(), 200);
    now.mutes.insert("Mic".into(), true);
    let back = state::apply_user_scene(&now, &sc);
    assert_eq!(back.volume("Chat"), 10);
    assert!(back.muted("Mic"), "les mutes ne font pas partie d'une scene");
    assert!(back.fx.gate);
}

#[test]
fn octets_envoyes() {
    let st = state::default_state();
    let r = routing::effective_routing(&st, "Music", false, false);
    let (l, rr) = routing::routing_bytes(&r, false);
    assert_eq!((l.len(), rr.len()), (22, 22));
    assert_eq!(routing::routing_bytes(&r, true).0.len(), 26);
    let mut m = st.clone();
    m.mutes.insert(m.fader_channel("A").to_string(), true);
    let map = lighting::colour_map(&m);
    assert_eq!(map.len(), 520);
    assert_eq!(&map[20 * 4..20 * 4 + 8], &[0u8; 8], "fader coupe : vumetre eteint");
    assert_eq!(lighting::button_states(&st, &Default::default(), false).len(), 24);
}

#[test]
fn preampli_comme_autoeq() {
    let (flt, pre) = hp_filters(&normalize_hp_eq(&json!({"headphone": "Sennheiser HD 560S"}), None));
    assert_eq!(flt.len(), 10);
    assert!((-7.2..=-6.6).contains(&pre), "{pre}");
    assert_eq!(hp_filters(&normalize_hp_eq(&json!({}), None)).1, 0.0);
}

#[test]
fn fichier_equalizer_apo() {
    let eq = normalize_hp_eq(&json!({"headphone": "Sennheiser HD 560S", "bands": [4, 5, 3, 1, 0, 0, 0, 0, 0, 0], "channels": ["Game", "Chat"]}), None);
    let t = config_text(&eq, true, "");
    assert!(t.is_ascii());
    assert!(t.contains("Device: GoXLR Game; GoXLR Chat"), "{t}");
    assert!(t.contains("Filter 1: ON LSC Fc 105 Hz Gain 9.0 dB Q 0.70"), "{t}");
    assert!(t.contains("Filter 11: ON PK Fc 31 Hz Gain 4.0 dB Q 1.41"), "{t}");
    assert!(t.trim_end().ends_with("Device: all"));
    assert!(!config_text(&eq, false, "live").contains("Filter"));
    assert_eq!(style("basses").unwrap().bands[0], 4);
}
