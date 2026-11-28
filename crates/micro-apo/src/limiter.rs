//! Deux blocs courts en rotation. Le pic du bloc suivant fixe le gain à atteindre à la fin
//! du bloc précédent. Ainsi l'attaque commence avant le pic et la voix garde sa continuité.
pub struct Limiter {
    delay: Vec<f32>,
    pos: usize,
    peak: f32,
    previous_peak: f32,
    gain: f32,
    release: f32,
}

impl Limiter {
    pub fn new(rate: u32) -> Self {
        let half = ((rate as f64 * 0.0015 - rustudio_core::micdsp::SUB as f64) / 2.)
            .round()
            .max(1.) as usize;
        Self {
            delay: vec![0.; 2 * half],
            pos: 0,
            peak: 0.,
            previous_peak: 0.,
            gain: 1.,
            release: 1. - (-(half as f32) / (rate as f32 * 0.060)).exp(),
        }
    }

    pub fn latency(&self) -> usize {
        self.delay.len()
    }

    pub fn next(&mut self, input: f32, ceiling: f32) -> f32 {
        let sample = std::mem::replace(&mut self.delay[self.pos], input);
        self.peak = self.peak.max(input.abs());
        self.pos = (self.pos + 1) % self.delay.len();
        let half = self.delay.len() / 2;
        if self.pos == 0 || self.pos == half {
            let required = |peak: f32| if peak > ceiling { ceiling / peak } else { 1. };
            let here = required(self.previous_peak);
            let target = here.min(required(self.peak));
            let start = self.gain.min(here);
            self.gain = if target < start {
                target
            } else {
                start + (target - start) * self.release
            };
            let inc = (self.gain - start) / half as f32;
            for (i, x) in self.delay[self.pos..self.pos + half].iter_mut().enumerate() {
                *x *= start + (i + 1) as f32 * inc;
            }
            self.previous_peak = std::mem::take(&mut self.peak);
        }
        // Un abaissement du plafond peut arriver alors qu'un bloc est déjà préparé.
        sample.clamp(-ceiling, ceiling)
    }

    pub fn recent_peak(&self) -> f32 {
        self.peak.max(self.previous_peak)
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }
}
