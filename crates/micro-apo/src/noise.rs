//! RNNoise reçoit exactement 480 échantillons à 48 kHz. L'horloge entière garde le même
//! nombre de trames par voie même sur une capture longue à une fréquence non multiple de 100.
use nnnoiseless::DenoiseState;

const FRAME: usize = DenoiseState::FRAME_SIZE;
const RATE: u32 = 48_000;
const TAPS: usize = 32;
const PHASES: usize = 128;

struct Resampler {
    input: u32,
    output: u32,
    phase: u32,
    table: Box<[[f32; TAPS]]>,
    ring: [f32; TAPS],
    pos: usize,
}

impl Resampler {
    fn new(input: u32, output: u32) -> Self {
        let cutoff = 0.94 * (f64::from(output) / f64::from(input)).min(1.0);
        let table = (0..PHASES)
            .map(|phase| {
                let mut kernel = std::array::from_fn(|tap| {
                    let x = tap as f64 - 15.0 - phase as f64 / PHASES as f64;
                    let a = std::f64::consts::PI * x;
                    let window = 0.42 + 0.5 * (a / 16.).cos() + 0.08 * (a / 8.).cos();
                    (window
                        * if a.abs() < 1e-9 {
                            cutoff
                        } else {
                            (a * cutoff).sin() / a
                        }) as f32
                });
                let sum: f32 = kernel.iter().sum();
                for k in &mut kernel {
                    *k /= sum;
                }
                kernel
            })
            .collect();
        Self {
            input,
            output,
            phase: 0,
            table,
            ring: [0.; TAPS],
            pos: 0,
        }
    }

    fn push(&mut self, sample: f32, mut emit: impl FnMut(f32)) {
        self.pos = (self.pos + 1) % TAPS;
        self.ring[self.pos] = sample;
        self.phase += self.output;
        while self.phase >= self.input {
            self.phase -= self.input;
            let weights = &self.table[(self.phase * PHASES as u32 / self.output) as usize];
            let mut sum = 0.;
            for (tap, weight) in weights.iter().enumerate() {
                sum += weight * self.ring[(self.pos + TAPS - tap) % TAPS];
            }
            emit(sum);
        }
    }
}

struct Frames {
    state: Box<DenoiseState<'static>>,
    input: [f32; FRAME],
    output: [f32; FRAME],
    pos: usize,
    hold: i32,
    gain: f32,
}

impl Frames {
    fn next(&mut self, x: f32, strength: i32, grace: i32) -> f32 {
        let result = self.output[self.pos];
        self.input[self.pos] = x.clamp(-1., 1.) * 32768.;
        self.pos += 1;
        if self.pos == FRAME {
            self.pos = 0;
            let speech = self.state.process_frame(&mut self.output, &self.input);
            if speech * 100. >= strength as f32 {
                self.hold = 1 + (grace + 9) / 10;
            }
            // Un bruit continu peut recevoir une probabilité de parole intermédiaire. Dans
            // les trames ouvertes, appliquer une atténuation progressive limitée à 8 dB.
            // Le maintien évite de couper les fins de syllabe, même à faible confiance.
            let confidence = speech.clamp(0., 1.);
            let target = if self.hold > 0 { 0.4 + 0.6 * confidence * confidence } else { 0. };
            self.hold = (self.hold - 1).max(0);
            let step = (target - self.gain) / FRAME as f32;
            for sample in &mut self.output {
                self.gain += step;
                *sample *= self.gain / 32768.;
            }
            self.gain = target;
        }
        result
    }
}

struct Conversion {
    up: Resampler,
    down: Resampler,
    queue: [f32; 16],
    read: usize,
    write: usize,
}

pub struct Noise {
    frames: Frames,
    conversion: Option<Conversion>,
}

impl Noise {
    pub fn new(rate: u32) -> Self {
        let conversion = (rate != RATE).then(|| Conversion {
            up: Resampler::new(rate, RATE),
            down: Resampler::new(RATE, rate),
            queue: [0.; 16],
            read: 0,
            write: rate.div_ceil(RATE) as usize,
        });
        Self {
            frames: Frames {
                state: DenoiseState::new(),
                input: [0.; FRAME],
                output: [0.; FRAME],
                pos: 0,
                hold: 0,
                gain: 0.,
            },
            conversion,
        }
    }

    pub fn latency(rate: u32) -> u32 {
        // Une trame de collecte et une de recouvrement RNNoise, puis les deux filtres de conversion.
        (2 * FRAME as u32 * rate).div_ceil(RATE)
            + if rate == RATE {
                0
            } else {
                15 + (15 * rate).div_ceil(RATE) + rate.div_ceil(RATE)
            }
    }

    pub fn next(&mut self, x: f32, strength: i32, grace: i32) -> f32 {
        let Some(c) = self.conversion.as_mut() else {
            return self.frames.next(x, strength, grace);
        };
        c.up.push(x, |a| {
            let b = self.frames.next(a, strength, grace);
            c.down.push(b, |y| {
                c.queue[c.write] = y;
                c.write = (c.write + 1) % c.queue.len();
            });
        });
        let y = c.queue[c.read];
        c.read = (c.read + 1) % c.queue.len();
        y
    }
}
