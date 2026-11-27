use rustudio_logitech_c920::sim::Sim;
use rustudio_logitech_c920::uvc;
use rustudio_logitech_c920::*;
use serde_json::json;

#[test]
fn lecture_initiale_bornes_xu_et_infos_c920() {
    let mut cam = Camera::new(Sim::default());
    let s = cam.read().unwrap();
    assert_eq!(s.device.vid, VID);
    assert_eq!(s.device.pid, PID_C920_2019);
    assert_eq!(s.device.revision, "0019");
    assert_eq!(s.device.firmware.as_deref(), Some("7.59.1035"));
    assert_eq!(s.device.dsp_platform.as_deref(), Some("PYTHON_V2B"));
    assert_eq!(s.brightness, 128);
    assert_eq!(s.contrast, 128);
    assert_eq!(s.power_line, PowerLineFreq::Hz60);
    assert_eq!(s.ranges.power_line.min, 1);
    assert_eq!(s.ranges.power_line.max, 2);
    // LowLightComp garde 0..1 malgre GetRange(19) == 0..0 sous usbvideo.sys.
    assert_eq!(s.ranges.low_light_comp.min, 0);
    assert_eq!(s.ranges.low_light_comp.max, 1);
    assert!(s.exposure_auto);
    assert!(s.low_light_comp);
    assert!(s.white_balance_auto);
    assert!(s.focus_auto);
    assert_eq!(s.zoom, 100);
    assert_eq!(s.led_mode, Some(LedMode::Auto));
    assert_eq!(cam.into_inner().writes, 0, "une simple lecture ne doit rien ecrire");
}

#[test]
fn validation_stricte_avant_ecriture_et_relecture_confirmee() {
    let mut cam = Camera::new(Sim::default());
    // Hors bornes : rien n'est ecrit sur la webcam.
    let err = cam.apply_json(&json!({"brightness": 999, "contrast": 140})).unwrap_err();
    assert!(err.contains("Luminosité"), "{err}");
    assert_eq!(cam.transport_mut().writes, 0);

    // Anti-scintillement Off (0) refuse sur 046D:0892 (plage reelle 1..=2).
    let err = cam.apply_json(&json!({"power_line": "off"})).unwrap_err();
    assert!(err.contains("Anti-scintillement"), "{err}");
    assert_eq!(cam.transport_mut().writes, 0);

    // Mauvais pas pour la mise au point (pas de 5).
    let err = cam.apply_json(&json!({"focus_auto": false, "focus": 13})).unwrap_err();
    assert!(err.contains("pas de 5"), "{err}");
    assert_eq!(cam.transport_mut().writes, 0);

    // Reglage inconnu refuse.
    assert!(cam.apply_json(&json!({"inconnu": 1})).is_err());
    assert_eq!(cam.transport_mut().writes, 0);

    // Application groupee validee et relue.
    let s = cam
        .apply_json(&json!({
            "brightness": 140,
            "contrast": 135,
            "saturation": 130,
            "sharpness": 145,
            "gain": 80,
            "white_balance_auto": false,
            "white_balance": 4500,
            "backlight_comp": true,
            "power_line": "hz50",
            "exposure_auto": false,
            "exposure": -6,
            "low_light_comp": false,
            "focus_auto": false,
            "focus": 35,
            "zoom": 130,
            "pan": -2,
            "tilt": 3,
            "led_mode": "off"
        }))
        .unwrap();

    assert_eq!(s.brightness, 140);
    assert_eq!(s.contrast, 135);
    assert_eq!(s.saturation, 130);
    assert_eq!(s.sharpness, 145);
    assert_eq!(s.gain, 80);
    assert!(!s.white_balance_auto);
    assert_eq!(s.white_balance, 4500);
    assert!(s.backlight_comp);
    assert_eq!(s.power_line, PowerLineFreq::Hz50);
    assert!(!s.exposure_auto);
    assert_eq!(s.exposure, -6);
    assert!(!s.low_light_comp);
    assert!(!s.focus_auto);
    assert_eq!(s.focus, 35);
    assert_eq!(s.zoom, 130);
    assert_eq!(s.pan, -2);
    assert_eq!(s.tilt, 3);
    assert_eq!(s.led_mode, Some(LedMode::Off));
    // Verifie que les octets 0, 2 et 4 de LED1 ont ete preserves (Read-Modify-Write).
    assert_eq!(cam.transport_mut().led1_bytes, [0x01, 0x00, 0x01, 0x00, 0x03]);
}

#[test]
fn ecrire_valeur_manuelle_coupe_auto_sans_erreur() {
    let mut cam = Camera::new(Sim::default());
    let s = cam.apply_json(&json!({"exposure": -7, "white_balance": 5000, "focus": 40})).unwrap();
    assert!(!s.exposure_auto);
    assert_eq!(s.exposure, -7);
    assert!(!s.white_balance_auto);
    assert_eq!(s.white_balance, 5000);
    assert!(!s.focus_auto);
    assert_eq!(s.focus, 40);

    let s = cam.apply_json(&json!({"exposure_auto": true, "white_balance_auto": true, "focus_auto": true})).unwrap();
    assert!(s.exposure_auto);
    assert!(s.white_balance_auto);
    assert!(s.focus_auto);
}

#[test]
fn presets_et_reinitialisation_usine() {
    let mut cam = Camera::new(Sim::default());
    let studio = cam.apply_preset("studio").unwrap();
    assert!(!studio.exposure_auto);
    assert!(!studio.low_light_comp);
    assert!(!studio.white_balance_auto);
    assert_eq!(studio.power_line, PowerLineFreq::Hz50);
    assert_eq!(studio.focus % 5, 0);

    for id in ["low-light", "natural", "cinema"] {
        cam.apply_preset(id).unwrap();
    }
    assert!(cam.apply_preset("inexistant").is_err());

    let reset = cam.reset_defaults().unwrap();
    assert_eq!(reset.brightness, reset.ranges.brightness.default);
    assert_eq!(reset.power_line, PowerLineFreq::Hz60);
    assert!(reset.exposure_auto);
    assert!(reset.low_light_comp);
    assert!(reset.white_balance_auto);
    assert!(reset.focus_auto);
    assert_eq!(reset.zoom, 100);
    assert_eq!(reset.led_mode, Some(LedMode::Auto));
}

#[test]
fn detection_derive_et_reapplication_ciblee() {
    let mut cam = Camera::new(Sim::default());
    let studio = cam.apply_preset("studio").unwrap();
    let desired = DesiredSettings::from(&studio);

    let (patch, fields) = desired.diff_from(&studio);
    assert!(patch.is_empty());
    assert!(fields.is_empty());

    // Simulation d'un reset externe (ex. OBS / Discord / veille Windows).
    cam.transport_mut().simulate_external_reset();
    let drifted = cam.read().unwrap();
    assert!(drifted.exposure_auto);
    assert!(drifted.low_light_comp);
    assert_eq!(drifted.power_line, PowerLineFreq::Hz60);

    let (patch, fields) = desired.diff_from(&drifted);
    assert!(!patch.is_empty());
    assert!(fields.contains(&"exposure_auto"));
    assert!(fields.contains(&"low_light_comp"));
    assert!(fields.contains(&"power_line"));
    assert!(fields.contains(&"white_balance_auto"));

    let restored = cam.apply_patch(&patch, false).unwrap();
    let (after_patch, after_fields) = desired.diff_from(&restored);
    assert!(after_patch.is_empty(), "derive restante : {after_fields:?}");
}

#[test]
fn mode_auto_ne_declenche_pas_de_fausse_derive_sur_valeur_manuelle() {
    let mut cam = Camera::new(Sim::default());
    let natural = cam.apply_preset("natural").unwrap();
    let desired = DesiredSettings::from(&natural);
    assert!(desired.exposure_auto && desired.white_balance_auto && desired.focus_auto);

    cam.transport_mut().values.insert(ControlId::Exposure, ControlValue { value: -8, auto: true });
    cam.transport_mut().values.insert(ControlId::WhiteBalance, ControlValue { value: 5600, auto: true });
    cam.transport_mut().values.insert(ControlId::Focus, ControlValue { value: 80, auto: true });

    let current = cam.read().unwrap();
    let (patch, fields) = desired.diff_from(&current);
    assert!(patch.is_empty(), "fausse derive en mode auto : {fields:?}");
}

#[test]
fn decodage_xu_logitech_exact_et_protection_lecture_seule() {
    // Verifie les octets exacts releves sur la vraie C920 046D:0892.
    assert_eq!(uvc::decode_firmware_version(&[0x3B, 0x07, 0x0B, 0x04]).unwrap(), "7.59.1035");
    assert_eq!(uvc::decode_device_identity(&[0x6D, 0x04, 0x92, 0x08, 0x19, 0x00]).unwrap(), (0x046D, 0x0892, 0x0019));
    assert_eq!(uvc::decode_dsp_platform(b"PYTHON_V2B\0\0\0\0\0\0").as_deref(), Some("PYTHON_V2B"));

    // Read-Modify-Write 5 octets LED1 (CS=9).
    let base = uvc::FACTORY_LED1_BYTES;
    assert_eq!(uvc::decode_led1(&base).unwrap(), (LedMode::Auto, 0));
    let off = uvc::patch_led1(&base, LedMode::Off, None).unwrap();
    assert_eq!(off, [0x01, 0x00, 0x01, 0x00, 0x03]);
    assert_eq!(uvc::decode_led1(&off).unwrap(), (LedMode::Off, 0));
    let blink = uvc::patch_led1(&base, LedMode::Blink, Some(40)).unwrap();
    assert_eq!(blink, [0x01, 0x02, 0x01, 40, 0x03]);
    assert_eq!(uvc::decode_led1(&blink).unwrap(), (LedMode::Blink, 40));
    assert!(uvc::decode_led1(&[0x01, 0x99, 0x01, 0x00, 0x03]).is_err());

    // Les selecteurs XU en lecture seule refusent toute ecriture.
    let mut sim = Sim::default();
    assert!(sim.set_xu(XuControl::FirmwareVersion, &[0, 0, 0, 0]).is_err());
    assert!(sim.set_xu(XuControl::DeviceIdentity, &[0; 6]).is_err());
    assert!(sim.set_xu(XuControl::DspPlatform, &[0; 16]).is_err());

    // Mapping KS exact (WhiteBalance=7, Powerline=13, AutoExposurePriority=19).
    assert_eq!(uvc::ks_property(ControlId::WhiteBalance).1, 7);
    assert_eq!(uvc::ks_property(ControlId::PowerLineFreq).1, 13);
    assert_eq!(uvc::ks_property(ControlId::LowLightComp).1, 19);
    assert!(uvc::uses_zero_flags(ControlId::PowerLineFreq));
    assert!(uvc::uses_zero_flags(ControlId::LowLightComp));
    assert!(!uvc::uses_zero_flags(ControlId::Brightness));

    let mut cam = Camera::new(sim);
    cam.transport_mut().disconnected = true;
    assert!(cam.read().is_err());
}

fn build_basic_support_buf(lists: &[(u32, u32, u32, u32, &[u8])]) -> Vec<u8> {
    let mut buf = vec![0u8; 40];
    // MembersListCount a l'offset 32 de KSPROPERTY_DESCRIPTION
    buf[32..36].copy_from_slice(&(lists.len() as u32).to_le_bytes());
    for &(members_flags, elem_size, count, flags, payload) in lists {
        buf.extend_from_slice(&members_flags.to_le_bytes());
        buf.extend_from_slice(&elem_size.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&flags.to_le_bytes());
        buf.extend_from_slice(payload);
    }
    buf
}

fn stepping_long_bytes(delta: u32, reserved: u32, min: i32, max: i32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&delta.to_le_bytes());
    b[4..8].copy_from_slice(&reserved.to_le_bytes());
    b[8..12].copy_from_slice(&min.to_le_bytes());
    b[12..16].copy_from_slice(&max.to_le_bytes());
    b
}

#[test]
fn decodage_ks_basicsupport_reel_usbvideo_sans_panic() {
    // Cas reel usbvideo.sys sur C920 046D:0892 : MembersFlags=1 (RANGES) avec MembersSize=16 (KSPROPERTY_STEPPING_LONG).
    let focus_payload = stepping_long_bytes(5, 0, 0, 250);
    let focus_buf = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_RANGES, 16, 1, 0, &focus_payload)]);
    let focus = uvc::parse_basic_support(&focus_buf).unwrap();
    assert_eq!(focus, uvc::BasicSupportRange { min: 0, max: 250, step: 5, default: None });

    // Brightness avec liste DEFAULT en second bloc.
    let bright_payload = stepping_long_bytes(1, 0, 0, 255);
    let bright_def = 128i32.to_le_bytes();
    let bright_buf = build_basic_support_buf(&[
        (uvc::KSPROPERTY_MEMBER_RANGES, 16, 1, 0, &bright_payload),
        (uvc::KSPROPERTY_MEMBER_VALUES, 4, 1, uvc::KSPROPERTY_MEMBER_FLAG_DEFAULT, &bright_def),
    ]);
    let bright = uvc::parse_basic_support(&bright_buf).unwrap();
    assert_eq!(bright, uvc::BasicSupportRange { min: 0, max: 255, step: 1, default: Some(128) });

    // PowerlineFrequency : 1..=2, pas 1.
    let pl_payload = stepping_long_bytes(1, 0, 1, 2);
    let pl_buf = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_RANGES, 16, 1, 0, &pl_payload)]);
    let pl = uvc::parse_basic_support(&pl_buf).unwrap();
    assert_eq!(pl, uvc::BasicSupportRange { min: 1, max: 2, step: 1, default: None });

    // LowLight (AutoExposurePriority) : [0, 0, 0, 0] -> step remonte a 1, min=0, max=0, zero panic.
    let ll_payload = stepping_long_bytes(0, 0, 0, 0);
    let ll_buf = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_RANGES, 16, 1, 0, &ll_payload)]);
    let ll = uvc::parse_basic_support(&ll_buf).unwrap();
    assert_eq!(ll, uvc::BasicSupportRange { min: 0, max: 0, step: 1, default: None });

    // Format 8 octets pur (KSPROPERTY_BOUNDS_LONG) avec MembersFlags=1.
    let mut bounds8 = [0u8; 8];
    bounds8[0..4].copy_from_slice(&(-11i32).to_le_bytes());
    bounds8[4..8].copy_from_slice(&(-2i32).to_le_bytes());
    let exp_buf = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_RANGES, 8, 1, 0, &bounds8)]);
    let exp = uvc::parse_basic_support(&exp_buf).unwrap();
    assert_eq!(exp, uvc::BasicSupportRange { min: -11, max: -2, step: 1, default: None });

    // Garde anti-panic si un pilote renvoie min > max.
    let bad_payload = stepping_long_bytes(1, 0, 200, 10);
    let bad_buf = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_STEPPEDRANGES, 16, 1, 0, &bad_payload)]);
    assert!(uvc::parse_basic_support(&bad_buf).is_none());

    // Tampon trop court ou tronque.
    assert!(uvc::parse_basic_support(&[0u8; 20]).is_none());
    assert!(uvc::parse_basic_support(&focus_buf[..focus_buf.len() - 4]).is_none());

    // Decodage DEFAULTVALUES (4 octets et KsVidCapS 40 octets) + topologie XU sans alignement.
    let def4 = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_VALUES, 4, 1, uvc::KSPROPERTY_MEMBER_FLAG_DEFAULT, &(-5i32).to_le_bytes())]);
    assert_eq!(uvc::parse_default_value(&def4), Some(-5));
    let mut vidcap40 = [0u8; 40];
    vidcap40[24..28].copy_from_slice(&128i32.to_le_bytes());
    let def40 = build_basic_support_buf(&[(uvc::KSPROPERTY_MEMBER_VALUES, 40, 1, uvc::KSPROPERTY_MEMBER_FLAG_DEFAULT, &vidcap40)]);
    assert_eq!(uvc::parse_default_value(&def40), Some(128));

    let mut topo = Vec::new();
    topo.extend_from_slice(&(8u32 + 3 * 16).to_le_bytes());
    topo.extend_from_slice(&3u32.to_le_bytes());
    for g in [uvc::PROPSETID_VIDCAP_VIDEOPROCAMP, uvc::KSNODETYPE_DEV_SPECIFIC, uvc::KSNODETYPE_DEV_SPECIFIC] {
        topo.extend_from_slice(&g.data1.to_le_bytes());
        topo.extend_from_slice(&g.data2.to_le_bytes());
        topo.extend_from_slice(&g.data3.to_le_bytes());
        topo.extend_from_slice(&g.data4);
    }
    // Decale volontairement d'un octet pour prouver l'absence d'exigence d'alignement.
    let mut unaligned = vec![0xAAu8];
    unaligned.extend_from_slice(&topo);
    assert_eq!(uvc::parse_topology_xu_nodes(&unaligned[1..]), vec![1, 2]);
}
