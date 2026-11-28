//! Adaptateur de flux du calcul de la GoXLR. Chaque voie garde ses propres mémoires.
//! Un changement de filtre est fondu sur 5 ms entre deux états stables.
use crate::noise::Noise;
use rustudio_core::{
    hpeq,
    micchain::{fixed, high_pass_at, smoothing_at},
    micdsp::*,
};
use rustudio_windows_mic::{control::Controls, BANDS};

/// Bornes de négociation du SDK audio Windows (AUDIO_MIN_FRAMERATE et AUDIO_MAX_FRAMERATE).
pub const RATES: std::ops::RangeInclusive<u32> = 10..=384_000;

#[derive(Clone, Copy)]
struct Design {
    params: Params,
    gain: f32,
    ceiling: f32,
}
impl Design {
    fn new(c: Controls, rate: u32) -> Self {
        let mut p = Params::default();
        if c.0[0] == 0 {
            return Self {
                params: p,
                gain: 1.,
                ceiling: f32::INFINITY,
            };
        }
        let a = c.0;
        let fs = f64::from(rate);
        let smooth = |ms| smoothing_at(ms, fs);
        if a[2] > 0 {
            p.hpf = fixed(high_pass_at(f64::from(a[2].clamp(40, 200)), 0.71, fs));
            p.flags |= FLAG_HPF;
        }
        for (i, band) in BANDS.iter().enumerate() {
            let db = a[i + 3].clamp(-9, 9);
            if db != 0 {
                p.flags |= FLAG_EQ0 << i;
                p.eq[i] = fixed(hpeq::biquad(band.kind, band.hz, f64::from(db), band.q, fs));
            }
        }
        let d = c.dynamics();
        if d.gate {
            p.flags |= FLAG_GATE;
            p.gate_threshold = d.threshold * 256;
            p.gate_hysteresis = 4 * 256;
            p.gate_range = d.depth * 256;
            p.gate_open = smooth(2.);
            p.gate_close = smooth(100.);
            p.gate_hold = (d.hold_ms as f64 * fs / (1000 * SUB) as f64).round() as i32;
        }
        if d.compressor {
            p.flags |= FLAG_COMP;
            p.comp_threshold = d.comp_db * 256;
            p.comp_slope = ((1. - 1. / f64::from(d.ratio)) * 65536.).round() as i32;
            p.comp_makeup = d.makeup * 256;
            p.comp_attack = smooth(d.attack_ms.into());
            p.comp_release = smooth(d.release_ms.into());
        }
        if d.deesser > 0 {
            p.flags |= FLAG_DEESS;
            p.deess_hp = fixed(high_pass_at(5000., std::f64::consts::FRAC_1_SQRT_2, fs));
            p.deess_max = (d.deesser as f64 * 25.6).round() as i32;
            p.deess_offset = 6 * 256;
            p.deess_floor = -50 * 256;
            p.deess_attack = smooth(1.);
            p.deess_release = smooth(40.);
            p.deess_scale = 2 * 256;
        }
        let ceiling = if d.limiter {
            10f32.powf(d.ceiling as f32 / 20.)
        } else {
            f32::INFINITY
        };
        Self {
            params: p,
            gain: 10f32.powf(a[1].clamp(-12, 12) as f32 / 20.),
            ceiling,
        }
    }

    fn render(&self, chain: &mut Chain, raw: [f32; SUB]) -> [f32; SUB] {
        if self.params.flags == 0 {
            return raw.map(|x| x * self.gain);
        }
        let mut fixed = raw.map(|x| {
            (f64::from(x * self.gain).clamp(-15.99, 15.99) * f64::from(FULL)).round() as i32
        });
        chain.process_quantum(&self.params, &mut fixed);
        fixed.map(|y| (f64::from(y) / f64::from(FULL)) as f32)
    }
}

struct Channel {
    chain: Chain,
    old: Chain,
    raw: [f32; SUB],
    output: [f32; SUB],
    noise: Option<Noise>,
    limiter: crate::limiter::Limiter,
}

pub struct Stream {
    rate: u32,
    lanes: Vec<Channel>,
    controls: Controls,
    current: Design,
    previous: Design,
    active_controls: Controls,
    transition: u32,
    ceiling_ramp: (f32, f32),
    fade: u32,
    pos: usize,
    /// Niveau de sortie puis réductions gate, compresseur, de-esser et limiteur, en dB × 256.
    pub levels: [i32; 5],
}

impl Stream {
    pub fn new(rate: u32, channels: usize, controls: Controls) -> Option<Self> {
        if !RATES.contains(&rate) || !(1..=2).contains(&channels) {
            return None;
        }
        let current = Design::new(controls, rate);
        let lanes = (0..channels)
            .map(|_| Channel {
                chain: Chain::default(),
                old: Chain::default(),
                raw: [0.; SUB],
                output: [0.; SUB],
                limiter: crate::limiter::Limiter::new(rate),
                noise: (controls.0[21] != 0).then(|| Noise::new(rate)),
            })
            .collect();
        Some(Self {
            rate,
            lanes,
            controls,
            current,
            previous: current,
            active_controls: controls,
            transition: 0,
            ceiling_ramp: (0., 0.),
            fade: (rate / 200).max(1),
            pos: 0,
            levels: [0; 5],
        })
    }

    pub fn latency(&self) -> u32 {
        (SUB + self.lanes[0].limiter.latency()) as u32
            + if self.lanes[0].noise.is_some() {
                Noise::latency(self.rate)
            } else {
                0
            }
    }

    pub fn update(&mut self, c: Controls) {
        // Le mode RNNoise fixe la latence négociée. Son activation rouvre le graphe hors du fil audio.
        self.controls = c;
    }

    fn start_update(&mut self) {
        if self.transition != 0 || self.controls.0[..21] == self.active_controls.0[..21] {
            return;
        }
        self.previous = self.current;
        self.current = Design::new(self.controls, self.rate);
        let headroom = self
            .lanes
            .iter()
            .map(|ch| ch.limiter.recent_peak())
            .fold(1., f32::max);
        self.ceiling_ramp = (
            self.previous.ceiling.min(headroom),
            self.current.ceiling.min(headroom),
        );
        self.active_controls = self.controls;
        for ch in &mut self.lanes {
            ch.old = ch.chain;
        }
        self.transition = self.fade;
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        let channels = self.lanes.len();
        let mut peak = 0f32;
        self.levels = [0; 5];
        for frame in buffer.chunks_exact_mut(channels) {
            if self.pos == 0 {
                self.start_update();
            }
            let ceiling = if self.transition != 0 && self.previous.ceiling != self.current.ceiling {
                let left =
                    self.transition.saturating_sub(self.pos as u32) as f32 / self.fade as f32;
                self.ceiling_ramp.1 + (self.ceiling_ramp.0 - self.ceiling_ramp.1) * left
            } else {
                self.current.ceiling
            };
            for (sample, ch) in frame.iter_mut().zip(&mut self.lanes) {
                let x = if sample.is_finite() {
                    sample.clamp(-16., 16.)
                } else {
                    0.
                };
                ch.raw[self.pos] = match &mut ch.noise {
                    Some(n) => n.next(
                        x,
                        self.controls.0[22].clamp(0, 99),
                        self.controls.0[23].clamp(0, 500),
                    ),
                    None => x,
                };
                *sample = ch.limiter.next(ch.output[self.pos], ceiling);
                peak = peak.max(sample.abs());
                if self.pos + 1 == SUB {
                    ch.output = self.current.render(&mut ch.chain, ch.raw);
                    if self.transition != 0 {
                        let old = self.previous.render(&mut ch.old, ch.raw);
                        for (i, (y, before)) in ch.output.iter_mut().zip(old).enumerate() {
                            let w =
                                self.transition.saturating_sub(i as u32) as f32 / self.fade as f32;
                            *y += (before - *y) * w;
                        }
                    }
                    for (v, reduction) in self.levels[1..].iter_mut().zip(ch.chain.reduction) {
                        *v = (*v).max(reduction);
                    }
                    ch.chain.reduction = [0; 4];
                }
            }
            self.pos = (self.pos + 1) % SUB;
            if self.pos == 0 {
                self.transition = self.transition.saturating_sub(SUB as u32);
            }
        }
        let gain = self
            .lanes
            .iter()
            .fold(1f32, |g, ch| g.min(ch.limiter.gain()));
        self.levels[4] = (-level_db((gain * (1u32 << 23) as f32) as u32)).max(0);
        self.levels[0] = level_db((peak * FULL as f32) as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_flux_flottant_restitue_le_vecteur_du_firmware_apres_le_delai_annonce() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../core/src/micdsp-vecteur.json")).unwrap();
        let words = |name: &str| -> Vec<i32> {
            reference[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_i64().unwrap() as i32)
                .collect()
        };
        let bytes: Vec<u8> = words("params")
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        let expected = words("output");
        let input = words("input");
        assert_eq!(input.len(), expected.len());
        let mut stream = Stream::new(48_000, 1, Controls::default()).unwrap();
        // Le firmware fournit ici directement ses coefficients fixes. Les tests de réponse
        // fréquentielle vérifient séparément leur construction depuis les réglages de l'appli.
        stream.current.params = Params::decode(&bytes).unwrap();
        let delay = stream.latency() as usize;
        let mut samples: Vec<f32> = input
            .iter()
            .map(|x| (f64::from(*x) / f64::from(FULL)) as f32)
            .collect();
        samples.resize(samples.len() + delay, 0.);
        for chunk in samples.chunks_mut(13) {
            stream.process(chunk);
        }
        assert!(samples[..delay].iter().all(|x| x.to_bits() == 0));
        let actual: Vec<i32> = samples[delay..]
            .iter()
            .map(|x| (f64::from(*x) * f64::from(FULL)).round() as i32)
            .collect();
        assert_eq!(actual, expected);
    }
}
