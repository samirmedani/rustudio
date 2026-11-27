//! Analyse audio locale : niveaux de toutes les voies, spectre d'une seule source en mode partagé.
//! Aucun son joué, aucun fichier audio créé ; les clients WASAPI sont fermés quand la page est quittée.
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub const BANDS: usize = 48;
const FFT: usize = 4096;
pub const FLOOR: f32 = -90.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub output: bool,
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub selected: Option<Source>,
    pub bands: Vec<f32>,
    pub peak_db: f32,
    pub levels: Vec<(String, Option<f32>)>,
    pub rate: u32,
    pub ready: bool,
    pub error: Option<String>,
    pub version: u64,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            selected: None,
            bands: vec![FLOOR; BANDS],
            peak_db: FLOOR,
            levels: vec![],
            rate: 0,
            ready: false,
            error: None,
            version: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    sources: Vec<Source>,
    selected: Option<Source>,
}

pub struct Monitor {
    tx: mpsc::Sender<Request>,
    frame: Arc<Mutex<Frame>>,
}
impl Monitor {
    pub fn new(simulated: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let frame = Arc::new(Mutex::new(Frame::default()));
        let shared = frame.clone();
        std::thread::Builder::new()
            .name("audio-spectrum".into())
            .spawn(move || {
                if simulated {
                    demo(rx, shared);
                    return;
                }
                #[cfg(windows)]
                native::run(rx, shared);
                #[cfg(not(windows))]
                {
                    let _ = rx;
                    publish(
                        &shared,
                        Frame {
                            error: Some("Analyse disponible sous Windows.".into()),
                            ..Frame::default()
                        },
                    );
                }
            })
            .expect("fil du spectre audio");
        Self { tx, frame }
    }
    pub fn configure(&self, sources: Vec<Source>, selected: Option<Source>) {
        let _ = self.tx.send(Request { sources, selected });
    }
    pub fn stop(&self) {
        self.configure(vec![], None);
    }
    pub fn frame(&self) -> Frame {
        self.frame.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
fn amplitude_db(amplitude: f32) -> f32 {
    if amplitude.is_finite() && amplitude > 0. {
        (20. * amplitude.log10()).clamp(FLOOR, 6.)
    } else {
        FLOOR
    }
}

/// Tampon borné, FFT de Hann ; puissances moyennées entre canaux, sans annuler une stéréo en opposition de phase.
struct Analyzer {
    channels: usize,
    ring: Vec<Vec<f64>>,
    at: usize,
    count: usize,
    window: Vec<f64>,
    gain: f64,
    re: Vec<f64>,
    im: Vec<f64>,
    power: Vec<f64>,
    band_ranges: Vec<Option<(usize, usize)>>,
}
impl Analyzer {
    fn new(rate: u32, channels: usize) -> Self {
        let window: Vec<_> = (0..FFT)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT as f64).cos())
            .collect();
        let gain = window.iter().sum::<f64>();
        let bin = rate as f64 / FFT as f64;
        let band_ranges = (0..BANDS)
            .map(|i| {
                let low = 20. * 1000f64.powf(i as f64 / BANDS as f64);
                let high = 20. * 1000f64.powf((i + 1) as f64 / BANDS as f64);
                if rate == 0 || low >= rate as f64 / 2. {
                    return None;
                }
                let a = (low / bin).round().max(1.) as usize;
                let b = ((high / bin).round() as usize).max(a).min(FFT / 2);
                (a <= b).then_some((a, b))
            })
            .collect();
        Self {
            channels,
            ring: vec![vec![0.; FFT]; channels],
            at: 0,
            count: 0,
            window,
            gain,
            re: vec![0.; FFT],
            im: vec![0.; FFT],
            power: vec![0.; FFT / 2 + 1],
            band_ranges,
        }
    }
    fn push(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.channels) {
            for (c, &v) in frame.iter().enumerate() {
                self.ring[c][self.at] = if v.is_finite() { v as f64 } else { 0. };
            }
            self.at = (self.at + 1) % FFT;
            self.count = (self.count + 1).min(FFT);
        }
    }
    fn spectrum(&mut self) -> Vec<f32> {
        if self.count < FFT {
            return vec![FLOOR; BANDS];
        }
        self.power.fill(0.);
        for channel in &self.ring {
            for i in 0..FFT {
                self.re[i] = channel[(self.at + i) % FFT] * self.window[i];
            }
            self.im.fill(0.);
            crate::soundcheck::fft(&mut self.re, &mut self.im, false);
            for (k, p) in self.power.iter_mut().enumerate() {
                *p += (self.re[k] * self.re[k] + self.im[k] * self.im[k])
                    * if k == 0 || k == FFT / 2 { 1. } else { 4. }
                    / (self.gain * self.gain * self.channels as f64);
            }
        }
        self.band_ranges
            .iter()
            .map(|range| {
                let Some((a, b)) = *range else {
                    return FLOOR;
                };
                let p = self.power[a..=b].iter().copied().fold(0., f64::max);
                if p > 0. {
                    (10. * p.log10()).clamp(FLOOR as f64, 6.) as f32
                } else {
                    FLOOR
                }
            })
            .collect()
    }
}

fn publish(shared: &Mutex<Frame>, mut next: Frame) {
    let mut f = shared.lock().unwrap_or_else(|e| e.into_inner());
    next.version = f.version + 1;
    *f = next;
}
fn demo(rx: mpsc::Receiver<Request>, shared: Arc<Mutex<Frame>>) {
    let start = Instant::now();
    let mut request = Request {
        sources: vec![],
        selected: None,
    };
    let mut phase = 0f64;
    let mut analyzer = Analyzer::new(48000, 2);
    loop {
        if request.sources.is_empty() {
            // Hors page Audio : aucun reveil tant qu'une source n'est pas demandee.
            let Ok(mut r) = rx.recv() else { break };
            while let Ok(last) = rx.try_recv() {
                r = last;
            }
            request = r;
            if request.sources.is_empty() {
                publish(&shared, Frame::default());
                continue;
            }
        } else {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(mut r) => {
                    while let Ok(last) = rx.try_recv() {
                        r = last;
                    }
                    request = r;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if request.sources.is_empty() {
                publish(&shared, Frame::default());
                continue;
            }
        }
        let amp = 0.12 + 0.1 * (start.elapsed().as_secs_f64() * 1.7).sin().abs();
        let mut samples = Vec::with_capacity(4800);
        for _ in 0..2400 {
            let v = (phase.sin() * amp) as f32;
            samples.extend([v, -v]);
            phase = (phase + std::f64::consts::TAU * 1000. / 48000.) % std::f64::consts::TAU;
        }
        analyzer.push(&samples);
        let levels = request
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    s.id.clone(),
                    Some(amplitude_db(
                        (amp * (0.25
                            + 0.75 * (start.elapsed().as_secs_f64() + i as f64).sin().abs()))
                            as f32,
                    )),
                )
            })
            .collect();
        publish(
            &shared,
            Frame {
                selected: request.selected.clone(),
                bands: analyzer.spectrum(),
                peak_db: amplitude_db(amp as f32),
                levels,
                rate: 48000,
                ready: request.selected.is_some(),
                ..Frame::default()
            },
        );
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::soundcheck::{
        to_f32,
        win::{self, Fmt},
    };
    use windows::Win32::{
        Media::Audio::{Endpoints::IAudioMeterInformation, IAudioCaptureClient, IAudioClient},
        System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    };
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    struct Capture {
        client: IAudioClient,
        cap: IAudioCaptureClient,
        format: Fmt,
        analyzer: Analyzer,
        last_data: Instant,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = unsafe { self.client.Stop() };
        }
    }
    impl Capture {
        unsafe fn open(source: &Source) -> Result<Self, String> {
            let device = win::device(&source.id)?;
            let (client, format) = win::open(&device, if source.output { 0x0002_0000 } else { 0 })?;
            if format.channels > 32 {
                return Err("Cette source possède trop de canaux pour l’analyse.".into());
            }
            let cap: IAudioCaptureClient = client.GetService().map_err(|e| e.to_string())?;
            client.Start().map_err(|e| e.to_string())?;
            let analyzer = Analyzer::new(format.rate, format.channels as usize);
            Ok(Self {
                client,
                cap,
                format,
                analyzer,
                last_data: Instant::now(),
            })
        }
        unsafe fn read(&mut self) -> Result<(Vec<f32>, f32), String> {
            let mut peak = 0f32;
            // Lecture bornée : l'interface et les demandes de fermeture restent réactives.
            for _ in 0..128 {
                if self.cap.GetNextPacketSize().map_err(|e| e.to_string())? == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                self.cap
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .map_err(|e| e.to_string())?;
                if frames == 0 {
                    break;
                }
                if data.is_null() && flags & 2 == 0 {
                    let _ = self.cap.ReleaseBuffer(frames);
                    return Err("Paquet audio sans données.".into());
                }
                let samples = if flags & 2 != 0 {
                    vec![0.; frames as usize * self.format.channels as usize]
                } else {
                    to_f32(
                        std::slice::from_raw_parts(
                            data,
                            frames as usize * self.format.block as usize,
                        ),
                        self.format.float,
                        self.format.block_bits,
                    )
                };
                self.cap.ReleaseBuffer(frames).map_err(|e| e.to_string())?;
                for &v in &samples {
                    if v.is_finite() {
                        peak = peak.max(v.abs());
                    }
                }
                self.analyzer.push(&samples);
                self.last_data = Instant::now();
            }
            if self.last_data.elapsed() > Duration::from_millis(250) {
                self.analyzer.count = 0;
                return Ok((vec![FLOOR; BANDS], FLOOR));
            }
            Ok((self.analyzer.spectrum(), amplitude_db(peak)))
        }
    }
    pub fn run(rx: mpsc::Receiver<Request>, shared: Arc<Mutex<Frame>>) {
        if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
            publish(
                &shared,
                Frame {
                    error: Some("Windows ne peut pas initialiser l’analyse audio.".into()),
                    ..Frame::default()
                },
            );
            return;
        }
        let _com = Com;
        let mut request = Request {
            sources: vec![],
            selected: None,
        };
        let mut meters: Vec<(String, Option<IAudioMeterInformation>, Instant)> = vec![];
        let mut capture = None;
        let mut frame = Frame::default();
        let mut retry = Instant::now();
        loop {
            let incoming = if request.sources.is_empty() {
                match rx.recv() {
                    Ok(r) => Some(r),
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(r) => Some(r),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };
            if let Some(mut r) = incoming {
                while let Ok(last) = rx.try_recv() {
                    r = last;
                }
                if r != request {
                    let changed = r.selected != request.selected;
                    let now = Instant::now();
                    // Réactiver les indicateurs sur le même fil COM, sans ouvrir de capture pour ces lignes.
                    meters = r
                        .sources
                        .iter()
                        .map(|s| {
                            let meter = unsafe {
                                win::device(&s.id)
                                    .ok()
                                    .and_then(|d| d.Activate(CLSCTX_ALL, None).ok())
                            };
                            let next_try = if meter.is_some() { now } else { now + Duration::from_secs(2) };
                            (s.id.clone(), meter, next_try)
                        })
                        .collect();
                    if changed {
                        capture = None;
                        frame = Frame {
                            selected: r.selected.clone(),
                            ..Frame::default()
                        };
                        retry = now;
                    }
                    request = r;
                }
            }
            if request.sources.is_empty() {
                capture = None;
                meters.clear();
                frame = Frame::default();
                publish(&shared, frame.clone());
                continue;
            }
            let now = Instant::now();
            if capture.is_none() && now >= retry {
                if let Some(source) = &request.selected {
                    match unsafe { Capture::open(source) } {
                        Ok(c) => {
                            frame.rate = c.format.rate;
                            frame.ready = true;
                            frame.error = None;
                            capture = Some(c);
                        }
                        Err(e) => {
                            frame.ready = false;
                            frame.error = Some(e);
                            retry = now + Duration::from_secs(2);
                        }
                    }
                }
            }
            frame.levels = meters
                .iter_mut()
                .map(|(id, m, next_try)| {
                    let level = m.as_ref().and_then(|m| unsafe { m.GetPeakValue().ok() });
                    if level.is_none() && now >= *next_try {
                        *m = unsafe {
                            win::device(id)
                                .ok()
                                .and_then(|d| d.Activate(CLSCTX_ALL, None).ok())
                        };
                        *next_try = now + Duration::from_secs(2);
                    }
                    (id.clone(), level.map(amplitude_db))
                })
                .collect();
            if let Some(c) = capture.as_mut() {
                match unsafe { c.read() } {
                    Ok((bands, peak)) => {
                        for (old, new) in frame.bands.iter_mut().zip(bands) {
                            *old = new.max(*old - 4.);
                        }
                        frame.peak_db = peak;
                    }
                    Err(e) => {
                        capture = None;
                        frame.ready = false;
                        frame.error = Some(e);
                        frame.bands.fill(FLOOR);
                        frame.peak_db = FLOOR;
                        retry = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
            publish(&shared, frame.clone());
        }
        // Les clients et indicateurs sont détruits avant CoUninitialize.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tone_frequency_level_and_antiphase_stereo() {
        for rate in [44100, 48000, 96000] {
            let mut a = Analyzer::new(rate, 2);
            let mut v = vec![];
            for i in 0..FFT {
                let x = (std::f64::consts::TAU * 1000. * i as f64 / rate as f64).sin() as f32 * 0.5;
                v.extend([x, -x]);
            }
            a.push(&v);
            let b = a.spectrum();
            let (i, level) = b
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap();
            let hz = 20. * 1000f64.powf((i as f64 + 0.5) / BANDS as f64);
            assert!((hz - 1000.).abs() < 160., "{hz}");
            assert!((*level + 6.).abs() < 2., "{level}");
        }
    }
    #[test]
    fn silence_short_buffers_and_nonfinite_are_quiet() {
        let mut a = Analyzer::new(48000, 2);
        a.push(&[f32::NAN, f32::INFINITY]);
        assert_eq!(a.spectrum(), vec![FLOOR; BANDS]);
        a.push(&vec![0.; FFT * 2]);
        assert_eq!(a.spectrum(), vec![FLOOR; BANDS]);
        assert_eq!(a.count, FFT);
    }
    #[test]
    fn nyquist_level_is_not_doubled() {
        let mut a = Analyzer::new(32000, 1);
        a.push(
            &(0..FFT)
                .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
                .collect::<Vec<_>>(),
        );
        let peak = a.spectrum().into_iter().fold(FLOOR, f32::max);
        assert!((peak + 6.0206).abs() < 0.05, "{peak}");
    }
    #[test]
    fn monitor_stops_and_switches_without_stale_source() {
        let m = Monitor::new(true);
        let s = Source {
            id: "demo-output".into(),
            output: true,
        };
        m.configure(vec![s.clone()], Some(s.clone()));
        let wait = |predicate: &dyn Fn(&Frame) -> bool| {
            let until = Instant::now() + Duration::from_secs(2);
            loop {
                let f = m.frame();
                if predicate(&f) {
                    return f;
                }
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&|f| f.ready && f.selected.as_ref() == Some(&s));
        m.stop();
        wait(&|f| !f.ready && f.levels.is_empty());
        let s = Source {
            id: "demo-input".into(),
            output: false,
        };
        m.configure(vec![s.clone()], Some(s.clone()));
        wait(&|f| f.selected.as_ref() == Some(&s));
    }
    #[test]
    fn monitor_au_repos_ne_reveille_pas() {
        let m = Monitor::new(true);
        let v0 = m.frame().version;
        std::thread::sleep(Duration::from_millis(180));
        assert_eq!(m.frame().version, v0, "aucun reveil tant qu'aucune source n'est demandee");
        let s = Source { id: "demo-output".into(), output: true };
        m.configure(vec![s.clone()], Some(s.clone()));
        let until = Instant::now() + Duration::from_secs(2);
        while !m.frame().ready {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        m.stop();
        let until = Instant::now() + Duration::from_secs(2);
        while m.frame().ready {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        let v_idle = m.frame().version;
        std::thread::sleep(Duration::from_millis(180));
        assert_eq!(m.frame().version, v_idle, "apres stop(), la version reste figee sans reveil 20 Hz");
    }
}
