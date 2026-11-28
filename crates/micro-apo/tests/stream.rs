//! Mesures entièrement en mémoire. Un seul test par fil est compté par l'allocateur instrumenté.
use rustudio_micro_apo::stream::Stream;
use rustudio_windows_mic::{control::Controls, Voice};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! { static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Count;
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|a| {
            if let Some(n) = a.get() {
                a.set(Some(n + 1))
            }
        });
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOCATOR: Count = Count;

fn controls(v: &Voice) -> Controls {
    Controls::new(v, false, 90, 200)
}
fn bare() -> Voice {
    Voice {
        enabled: true,
        hpf: false,
        ..Default::default()
    }
}
fn response(v: &Voice, rate: u32, level: f32, hz: f32) -> f32 {
    let mut s = Stream::new(rate, 1, controls(v)).unwrap();
    let mut x: Vec<_> = (0..rate)
        .map(|i| level * (i as f32 * std::f32::consts::TAU * hz / rate as f32).sin())
        .collect();
    s.process(&mut x);
    10. * (2. * x[x.len() / 2..].iter().map(|x| x * x).sum::<f32>()
        / (x.len() - x.len() / 2) as f32)
        .log10()
}

#[test]
fn mesures_du_gate_compresseur_egaliseur_coupe_bas_et_deesser() {
    for rate in [16000, 44100, 48000, 96000] {
        let mut v = bare();
        v.dynamics.gate = true;
        assert!(response(&v, rate, 0.001, 300.) < -110., "gate {rate}");
        assert!((response(&v, rate, 0.25, 300.) + 12.04).abs() < 0.15);
        v.dynamics.gate = false;
        v.dynamics.compressor = true;
        v.dynamics.comp_db = -24;
        v.dynamics.ratio = 4;
        let loud = response(&v, rate, 1., 1200.);
        assert!((-18.5..-17.).contains(&loud), "comp {rate}: {loud}");
        v.dynamics.compressor = false;
        v.eq[2] = 6;
        assert!((response(&v, rate, 0.1, 1000.) + 14.).abs() < 0.2);
        v.eq = [0; 5];
        v.hpf = true;
        v.hpf_hz = 120;
        assert!(response(&v, rate, 0.1, 30.) < -43.);
        v.hpf = false;
        v.dynamics.deesser = 90;
        assert!(response(&v, rate, 0.1, 7000.) < -24.);
    }
}

#[test]
fn retard_mesure_pic_plafonne_et_independance_des_paquets() {
    for rate in [
        8000, 11025, 16000, 44100, 48000, 88200, 96000, 192000, 352800, 384000,
    ] {
        let mut v = bare();
        v.dynamics.limiter = true;
        v.dynamics.ceiling = -6;
        let mut s = Stream::new(rate, 2, controls(&v)).unwrap();
        let delay = s.latency() as usize;
        assert!((delay as f64 / rate as f64 - 0.0015).abs() <= 1.1 / rate as f64);
        let mut x = vec![0.; (rate / 7) as usize * 2];
        x[0] = 0.25;
        // La première impulsion mesure le délai au gain unitaire. À 384 kHz,
        // 445 trames tombent déjà dans l’anticipation du pic suivant.
        let isolated_peak = 445.max(delay * 2);
        x[isolated_peak * 2] = 1.;
        x[(isolated_peak + 1) * 2] = -2.;
        let mut y = x.clone();
        s.process(&mut x);
        let mut t = Stream::new(rate, 2, controls(&v)).unwrap();
        for block in y.chunks_mut(14) {
            t.process(block)
        }
        assert_eq!(x, y, "frontière de bloc {rate}");
        assert_eq!(x.iter().position(|v| *v != 0.), Some(delay * 2));
        assert_eq!(x[delay * 2], 0.25);
        assert!(x.iter().all(|v| v.abs() <= 10f32.powf(-6. / 20.) + 1e-6));
        assert!(x.iter().skip(1).step_by(2).all(|&v| v == 0.));
    }
}

#[test]
fn calcul_commun_identique_au_vecteur_du_firmware_a_48khz() {
    use rustudio_core::micdsp::{Chain, Params, FULL, SUB};
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../core/src/micdsp-vecteur.json")).unwrap();
    let bytes: Vec<_> = data["params"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|v| (v.as_i64().unwrap() as u32).to_le_bytes())
        .collect();
    let params = Params::decode(&bytes).unwrap();
    let mut chain = Chain::default();
    let mut checked = 0;
    let values = |k: &str| {
        data[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect::<Vec<_>>()
    };
    let input = values("input");
    let expected = values("output");
    for (x, y) in input.chunks_exact(SUB).zip(expected.chunks_exact(SUB)) {
        let mut b: [i32; SUB] = x.try_into().unwrap();
        chain.process_quantum(&params, &mut b);
        assert_eq!(b.map(|v| v.clamp(-FULL, FULL)), y);
        checked += SUB;
    }
    assert_eq!(checked, expected.len());
    assert!(checked > 0);
}

#[test]
fn mise_a_jour_en_cours_de_phrase_sans_saut_de_gain() {
    let mut v = bare();
    let mut s = Stream::new(48000, 1, controls(&v)).unwrap();
    let mut first = [0.1; 1024];
    s.process(&mut first);
    v.gain_db = 12;
    s.update(controls(&v));
    let mut next = [0.1; 1024];
    s.process(&mut next);
    assert!(next.windows(2).all(|w| (w[0] - w[1]).abs() < 0.003));
    assert!((next[1023] - 0.3981).abs() < 0.001);
}

#[test]
fn rnnoise_multifrequence_et_reglages_aleatoires_sans_allocation_audio() {
    for rate in [8000, 44100, 48000, 96000, 192000, 384000] {
        let mut v = bare();
        v.apply_preset("streamer").unwrap();
        let mut s = Stream::new(rate, 2, Controls::new(&v, true, 60, 100)).unwrap();
        let mut buffer = [0.; 192];
        let mut seed = 12345u32;
        ALLOCS.with(|a| a.set(Some(0)));
        for i in 0..500 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if i % 9 == 0 {
                v.dynamics.comp_db = -(3 + (seed % 45) as i32);
                v.eq[0] = (seed % 19) as i32 - 9;
                s.update(Controls::new(&v, true, 60, 100));
            }
            for (n, x) in buffer.iter_mut().enumerate() {
                *x = if n == 17 {
                    f32::NAN
                } else if n == 71 {
                    f32::INFINITY
                } else {
                    ((seed.rotate_left(n as u32) as i32) as f32) / (i32::MAX as f32)
                };
            }
            s.process(&mut buffer);
            assert!(buffer.iter().all(|x| x.is_finite() && x.abs() <= 1.));
        }
        let allocs = ALLOCS.with(|a| a.replace(None));
        assert_eq!(allocs, Some(0), "allocation sur le fil audio {rate} Hz");
    }
}

#[test]
fn le_dernier_reglage_rapide_arrive_sans_interrompre_le_fondu() {
    let mut v = bare();
    let mut s = Stream::new(48000, 1, controls(&v)).unwrap();
    s.process(&mut [0.1; 512]);
    let mut previous = 0.1;
    for value in [12, -12, 10, -8, 7, -6, 12] {
        v.gain_db = value;
        s.update(controls(&v));
        let mut block = [0.1; 41];
        s.process(&mut block);
        for x in block {
            assert!((x - previous).abs() < 0.003);
            previous = x;
        }
    }
    let mut settle = [0.1; 1024];
    s.process(&mut settle);
    assert!((settle[1023] - 0.3981).abs() < 0.001);
}

#[test]
fn rnnoise_filtre_le_bruit_et_conserve_le_retard_annonce_sur_une_voix_synthetique() {
    for rate in [16000, 44100, 48000, 96000] {
        let voice = bare();
        let n = rate as usize;
        let mut seed = 7u32;
        let mut source = Vec::with_capacity(n);
        for i in 0..n {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let t = i as f64 / f64::from(rate);
            let phase = 2. * std::f64::consts::PI * (107. * t + 45. * t * t);
            let voiced = (1..=20)
                .map(|h| {
                    let h = h as f64;
                    (h * phase).sin()
                        * (0.55 * (-((h * 140. - 780.) / 240.).powi(2)).exp()
                            + 0.35 * (-((h * 140. - 1640.) / 290.).powi(2)).exp()
                            + 0.10)
                        / h.sqrt()
                })
                .sum::<f64>();
            let white = seed as f64 / f64::from(u32::MAX) - 0.5;
            source.push(
                (if t < 0.35 {
                    0.15 * white
                } else {
                    0.34 * voiced + 0.01 * white
                }) as f32,
            );
        }
        let mut out = source.clone();
        let mut stream = Stream::new(rate, 1, Controls::new(&voice, true, 0, 200)).unwrap();
        let expected = stream.latency() as usize;
        for b in out.chunks_mut(134) {
            stream.process(b);
        }
        let energy = |x: &[f32]| x.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
        let noise = n / 6..n / 3;
        assert!(
            energy(&out[noise.clone()]) < energy(&source[noise]) * 0.5,
            "réduction de bruit à {rate}"
        );
        let a = n / 2;
        let b = n - expected - 32;
        let corr = |lag: usize| {
            let x = &source[a..b];
            let y = &out[a + lag..b + lag];
            x.iter()
                .zip(y)
                .map(|(&u, &v)| f64::from(u) * f64::from(v))
                .sum::<f64>()
                / (energy(x) * energy(y)).sqrt()
        };
        let measured = (expected - 20..=expected + 20)
            .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
            .unwrap();
        assert!(
            corr(measured) > 0.65,
            "voix à {rate} Hz, corrélation {}",
            corr(measured)
        );
        assert!(
            expected.abs_diff(measured) <= 3,
            "{rate} Hz attendu {expected} mesuré {measured}"
        );
        println!(
            "RNNoise {rate} Hz : {measured} échantillons mesurés, corrélation {:.3}",
            corr(measured)
        );
    }
}

#[test]
fn le_plafond_du_limiteur_se_regle_sans_marche_en_pleine_voix() {
    let mut v = bare();
    let mut s = Stream::new(48000, 1, controls(&v)).unwrap();
    let mut previous = 0.8;
    s.process(&mut [0.8; 2048]);
    for ceiling in [-8, -12, -2] {
        v.dynamics.limiter = true;
        v.dynamics.ceiling = ceiling;
        s.update(controls(&v));
        let mut b = [0.8; 960];
        s.process(&mut b);
        for &x in &b {
            assert!((x - previous).abs() < 0.007, "saut {previous} à {x}");
            previous = x;
        }
        assert!(b[480..]
            .iter()
            .all(|x| x.abs() <= 10f32.powf(ceiling as f32 / 20.) + 1e-6));
    }
}

#[test]
fn formats_aux_bornes_du_sdk_et_frequences_atypiques() {
    let v = bare();
    for rate in [0, 9, 384001, u32::MAX] {
        assert!(Stream::new(rate, 1, controls(&v)).is_none());
    }
    for rate in [10, 37, 7333, 12345, 176400, 384000] {
        let mut s = Stream::new(rate, 1, Controls::new(&v, true, 85, 200)).unwrap();
        let mut x = [0.125; 32];
        s.process(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));
    }
}

#[test]
fn le_limiteur_commence_a_attenuer_avant_le_pic() {
    for rate in [48000, 192000, 384000] {
        let mut v = bare();
        v.dynamics.limiter = true;
        v.dynamics.ceiling = -6;
        let mut s = Stream::new(rate, 1, controls(&v)).unwrap();
        let delay = s.latency() as usize;
        let pulse = delay * 4;
        let mut b = vec![0.25; delay * 7];
        b[pulse] = 2.;
        s.process(&mut b);
        assert_eq!(b[delay * 2], 0.25);
        assert!(b[pulse + delay - 1] < 0.23, "anticipation à {rate}");
        assert!(b[pulse + delay] <= 10f32.powf(-6. / 20.) + 1e-6);
    }
}
