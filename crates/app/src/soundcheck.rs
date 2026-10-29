//! Test « son qui crépite ». Deux mesures, en même temps :
//! - écoute : chaque voie GoXLR, sans rien jouer. Entrées (Chat Mic, Broadcast Stream Mix, VOD) : données perdues
//!   entre la GoXLR et Windows, micro qui sature. Sorties (System, Game, Music, Chat), en « loopback » : niveau
//!   maximum après l'égaliseur du casque, échantillons à 0 dBFS ou au-delà ;
//! - boucle : un son aigu discret est joué sur la voie Sample, que le moteur envoie seulement vers le Stream Mix
//!   (ni casque, ni Discord) pendant le test, puis réécouté par le Stream Mix. Le moindre trou entre le PC et la
//!   GoXLR, dans un sens ou dans l'autre, coupe ou décale ce son : on le voit à la milliseconde.
use serde_json::{json, Value};

/// Ce qu'une voie a donné pendant le test.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub frames: u64,
    pub packets: u64,
    /// discontinuités signalées par Windows (hors premier paquet)
    pub discontinuities: u32,
    /// trous dans la position du périphérique (entrées seulement)
    pub gaps: u32,
    pub gap_frames: u64,
    pub peak: f32,
    /// échantillons à pleine échelle (|x| >= 0,9999)
    pub clipped: u64,
    /// échantillons au-delà de la pleine échelle (sortie flottante de Windows, avant conversion)
    pub overs: u64,
    pub sound_frames: u64,
    pub sample_rate: u32,
    /// niveau maximum vu par l'indicateur de Windows (panneau Son), 0 à 1
    pub meter_peak: f32,
}

impl Stats {
    /// Ajoute un paquet de \`frames\` trames ; \`samples\` déjà en flottants (vide si silencieux).
    pub fn add(&mut self, frames: u32, flags: u32, devpos: Option<u64>, expected: Option<u64>, samples: &[f32], track_gaps: bool) {
        if self.packets > 0 && flags & 1 != 0 {
            self.discontinuities += 1;
        }
        if track_gaps {
            if let (Some(p), Some(e)) = (devpos, expected) {
                if p > e {
                    self.gaps += 1;
                    self.gap_frames += p - e;
                }
            }
        }
        self.packets += 1;
        self.frames += frames as u64;
        let mut loud = false;
        for &x in samples {
            let a = x.abs();
            if a > self.peak {
                self.peak = a;
            }
            if a >= 0.9999 {
                self.clipped += 1;
            }
            if a > 1.0001 {
                self.overs += 1;
            }
            loud |= a > 0.0005; // au-dessus de -66 dBFS
        }
        if loud {
            self.sound_frames += frames as u64;
        }
    }
    pub fn peak_db(&self) -> Option<f64> {
        db(self.peak)
    }
}

fn db(x: f32) -> Option<f64> {
    (x > 0.0).then(|| (20.0 * (x as f64).log10() * 10.0).round() / 10.0)
}

/// Conversion d'un tampon brut (format de mixage de Windows) en flottants.
pub fn to_f32(raw: &[u8], float: bool, block_bits: u16) -> Vec<f32> {
    match (float, block_bits) {
        (true, 32) => raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        (false, 16) => raw.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        (false, 24) => raw.chunks_exact(3).map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0).collect(),
        (false, 32) => raw.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0).collect(),
        _ => vec![],
    }
}

/// Verdict lisible pour une voie écoutée.
pub fn verdict(name: &str, loopback: bool, secs: f64, s: &Stats) -> Value {
    let lost = s.discontinuities as u64 + s.gaps as u64;
    let expected = (s.sample_rate as f64 * secs) as u64;
    // une entrée qui livre nettement moins que prévu a perdu des données, même sans les signaler
    let short = !loopback && s.sample_rate > 0 && expected > 0 && s.frames + s.sample_rate as u64 / 5 < expected;
    let status = if s.packets == 0 && !loopback {
        "silent"
    } else if lost > 0 || short {
        "dropouts"
    } else if loopback && (s.overs > 0 || s.clipped > 2) && s.sound_frames > 0 {
        "clipping"
    } else if !loopback && s.clipped > 2 {
        "clipping"
    } else {
        "ok"
    };
    json!({
        "name": name, "kind": if loopback { "output" } else { "input" }, "status": status,
        "frames": s.frames, "expected_frames": expected, "discontinuities": s.discontinuities, "gaps": s.gaps,
        "gap_ms": if s.sample_rate > 0 { (s.gap_frames as f64 * 1000.0 / s.sample_rate as f64).round() } else { 0.0 },
        "peak_db": s.peak_db(), "clipped": s.clipped, "overs": s.overs,
        "active_s": if s.sample_rate > 0 { (s.sound_frames as f64 / s.sample_rate as f64 * 10.0).round() / 10.0 } else { 0.0 },
        "meter_db": db(s.meter_peak),
    })
}

// ---------- test en boucle ----------

/// Fréquence du son de test : aiguë (loin de la voix) et sans nombre entier de périodes par milliseconde,
/// pour qu'un paquet USB perdu (1 ms) fasse sauter la phase.
pub const LOOP_FREQ: f64 = 14_750.0;
/// Niveau du son de test (-20 dBFS).
#[allow(dead_code)]
pub const LOOP_AMP: f32 = 0.1;
/// Bloc d'analyse (2 ms à 48 kHz).
const LOOP_BLOCK: usize = 96;

/// Démodulation du son de test, bloc par bloc (amplitude, phase), au fil de l'enregistrement.
pub struct ToneBlocks {
    w: f64,
    n: u64,
    i: f64,
    q: f64,
    count: usize,
    pub blocks: Vec<(f64, f64)>,
}

impl ToneBlocks {
    pub fn new(fs: f64) -> ToneBlocks {
        ToneBlocks { w: 2.0 * std::f64::consts::PI * LOOP_FREQ / fs, n: 0, i: 0.0, q: 0.0, count: 0, blocks: vec![] }
    }
    pub fn push(&mut self, v: f32) {
        let ph = self.w * self.n as f64;
        self.i += v as f64 * ph.cos();
        self.q -= v as f64 * ph.sin();
        self.n += 1;
        self.count += 1;
        if self.count == LOOP_BLOCK {
            let (i, q) = (self.i * 2.0 / LOOP_BLOCK as f64, self.q * 2.0 / LOOP_BLOCK as f64);
            self.blocks.push(((i * i + q * q).sqrt(), q.atan2(i)));
            self.i = 0.0;
            self.q = 0.0;
            self.count = 0;
        }
    }
}

/// Accidents du son de test : amplitude qui chute (trou) ou phase qui saute (échantillons perdus ou en trop).
/// Le début et la fin (son pas encore arrivé, ou déjà coupé) sont écartés.
/// Renvoie (blocs analysés, amplitude médiane, événements [(instant s, type)]).
pub fn analyse_blocks(blocks: &[(f64, f64)], fs: f64) -> (usize, f64, Vec<(f64, &'static str)>) {
    let mut amps: Vec<f64> = blocks.iter().map(|b| b.0).collect();
    amps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = amps.get(amps.len() / 2).copied().unwrap_or(0.0);
    if blocks.len() < 8 || med < 1e-4 {
        return (0, med, vec![]);
    }
    let first = blocks.iter().position(|b| b.0 > med * 0.5).unwrap_or(0) + 3;
    let last = blocks.iter().rposition(|b| b.0 > med * 0.5).unwrap_or(0).saturating_sub(3);
    let mut events: Vec<(f64, &'static str)> = vec![];
    let mut last_event = -10i64;
    for k in first.max(1)..=last {
        let (a, p) = blocks[k];
        let mut d = p - blocks[k - 1].1;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        let kind = if a < med * 0.5 {
            Some("dropout")
        } else if d.abs() > 0.35 && blocks[k - 1].0 > med * 0.5 {
            Some("jump")
        } else {
            None
        };
        if let Some(kind) = kind {
            // un même accident touche souvent 2 ou 3 blocs voisins : un seul événement
            if k as i64 - last_event > 3 {
                events.push(((k * LOOP_BLOCK) as f64 / fs, kind));
            }
            last_event = k as i64;
        }
    }
    (last.saturating_sub(first) + 1, med, events)
}

#[allow(dead_code)]
pub fn analyse_tone(x: &[f32], fs: f64) -> (usize, f64, Vec<(f64, &'static str)>) {
    let mut t = ToneBlocks::new(fs);
    for v in x {
        t.push(*v);
    }
    analyse_blocks(&t.blocks, fs)
}

/// Verdict du test en boucle.
pub fn loop_verdict(blocks: &[(f64, f64)], fs: f64, underruns: u32, capture: &Value) -> Value {
    let (n, med, events) = analyse_blocks(blocks, fs);
    let status = if n == 0 {
        "no-signal"
    } else if underruns > 0 {
        "test-late"
    } else if events.is_empty() {
        "ok"
    } else {
        "glitches"
    };
    json!({
        "status": status, "analysed_s": (n * LOOP_BLOCK) as f64 / fs, "level_db": db(med as f32),
        "dropouts": events.iter().filter(|e| e.1 == "dropout").count(),
        "jumps": events.iter().filter(|e| e.1 == "jump").count(),
        "events": events.iter().take(50).map(|(t, k)| json!({"t": (t * 1000.0).round() / 1000.0, "kind": k})).collect::<Vec<_>>(),
        "self_underruns": underruns, "capture": capture,
    })
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::audio::Endpoint;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{
        IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
        WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

    const LOOPBACK: u32 = 0x0002_0000;
    const FLOAT_SUBTYPE: [u8; 16] = [3, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71];

    struct Fmt {
        float: bool,
        rate: u32,
        block: u16,
        channels: u16,
        block_bits: u16,
    }

    unsafe fn device(id: &str) -> Result<IMMDevice, String> {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let w: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        e.GetDevice(PCWSTR(w.as_ptr())).map_err(|e| e.to_string())
    }

    /// Ouvre la voie en mode partagé (200 ms de tampon) dans son format de mixage ; en mode exclusif (diagnostic :
    /// sans le mixeur ni les effets de Windows), en 48 kHz, 24 bits dans 32, stéréo, le format des voies GoXLR.
    unsafe fn open(dev: &IMMDevice, flags: u32) -> Result<(IAudioClient, Fmt), String> {
        open_mode(dev, flags, false)
    }
    unsafe fn open_mode(dev: &IMMDevice, flags: u32, exclusive: bool) -> Result<(IAudioClient, Fmt), String> {
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
        if exclusive {
            let mut ext = [0u8; 40];
            ext[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
            ext[2..4].copy_from_slice(&2u16.to_le_bytes());
            ext[4..8].copy_from_slice(&48_000u32.to_le_bytes());
            ext[8..12].copy_from_slice(&384_000u32.to_le_bytes());
            ext[12..14].copy_from_slice(&8u16.to_le_bytes());
            ext[14..16].copy_from_slice(&32u16.to_le_bytes());
            ext[16..18].copy_from_slice(&22u16.to_le_bytes());
            ext[18..20].copy_from_slice(&24u16.to_le_bytes());
            ext[20..24].copy_from_slice(&3u32.to_le_bytes());
            ext[24..40].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]);
            client
                .Initialize(windows::Win32::Media::Audio::AUDCLNT_SHAREMODE_EXCLUSIVE, flags, 2_000_000, 0, ext.as_ptr() as *const WAVEFORMATEX, None)
                .map_err(|e| format!("mode exclusif : {e}"))?;
            return Ok((client, Fmt { float: false, rate: 48_000, block: 8, channels: 2, block_bits: 32 }));
        }
        let fmt = client.GetMixFormat().map_err(|e| e.to_string())?;
        let wf: WAVEFORMATEX = std::ptr::read_unaligned(fmt);
        let (tag, rate, block, channels) = (wf.wFormatTag, wf.nSamplesPerSec, wf.nBlockAlign, wf.nChannels);
        let float = tag == 3 || (tag == 0xFFFE && wf.cbSize >= 22 && std::slice::from_raw_parts((fmt as *const u8).add(24), 16) == FLOAT_SUBTYPE);
        let block_bits = if channels > 0 { block / channels * 8 } else { 0 };
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 2_000_000, 0, fmt, None);
        CoTaskMemFree(Some(fmt as *const core::ffi::c_void));
        init.map_err(|e| e.to_string())?;
        Ok((client, Fmt { float, rate, block, channels, block_bits }))
    }

    /// Écoute une voie \`secs\` secondes ; chaque paquet passe aussi par \`sink\` (échantillons entrelacés, voies).
    unsafe fn listen(id: &str, loopback: bool, secs: f64, sink: &mut dyn FnMut(&[f32], u16)) -> Result<Stats, String> {
        let dev = device(id)?;
        let (client, f) = open(&dev, if loopback { LOOPBACK } else { 0 })?;
        let meter: Option<IAudioMeterInformation> = dev.Activate(CLSCTX_ALL, None).ok();
        let cap: IAudioCaptureClient = client.GetService().map_err(|e| e.to_string())?;
        let mut st = Stats { sample_rate: f.rate, ..Default::default() };
        let mut next: Option<u64> = None;
        client.Start().map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < secs {
            std::thread::sleep(Duration::from_millis(10));
            if let Some(v) = meter.as_ref().and_then(|m| m.GetPeakValue().ok()) {
                st.meter_peak = st.meter_peak.max(v);
            }
            loop {
                let n = match cap.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(e) => {
                        let _ = client.Stop();
                        return Err(e.to_string());
                    }
                };
                if n == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags, mut pos, mut qpc) = (std::ptr::null_mut(), 0u32, 0u32, 0u64, 0u64);
                if cap.GetBuffer(&mut data, &mut frames, &mut flags, Some(&mut pos), Some(&mut qpc)).is_err() {
                    break;
                }
                let samples = if flags & 2 != 0 || data.is_null() {
                    vec![0.0; frames as usize * f.channels as usize]
                } else {
                    to_f32(std::slice::from_raw_parts(data, frames as usize * f.block as usize), f.float, f.block_bits)
                };
                st.add(frames, flags, Some(pos), next, if flags & 2 != 0 { &[] } else { &samples }, !loopback);
                sink(&samples, f.channels);
                next = Some(pos + frames as u64);
                let _ = cap.ReleaseBuffer(frames);
            }
        }
        let _ = client.Stop();
        Ok(st)
    }

    /// Joue le son de test sur une voie jusqu'à \`stop\` ; renvoie le nombre de fois où notre propre tampon s'est vidé.
    unsafe fn play_tone(id: &str, stop: &AtomicBool, exclusive: bool) -> Result<u32, String> {
        let dev = device(id)?;
        let (client, f) = open_mode(&dev, 0, exclusive)?;
        let render: IAudioRenderClient = client.GetService().map_err(|e| e.to_string())?;
        let size = client.GetBufferSize().map_err(|e| e.to_string())?;
        let step = 2.0 * std::f64::consts::PI * LOOP_FREQ / f.rate as f64;
        let mut phase = 0.0f64;
        let fill = |n: u32, phase: &mut f64| -> Result<(), String> {
            if n == 0 {
                return Ok(());
            }
            let p = render.GetBuffer(n).map_err(|e| e.to_string())?;
            let out = std::slice::from_raw_parts_mut(p, n as usize * f.block as usize);
            let bytes = (f.block_bits / 8) as usize;
            for fr in 0..n as usize {
                let v = LOOP_AMP * (*phase).sin() as f32;
                *phase = (*phase + step) % (2.0 * std::f64::consts::PI);
                for c in 0..f.channels as usize {
                    let o = &mut out[(fr * f.channels as usize + c) * bytes..][..bytes];
                    match (f.float, bytes) {
                        (true, 4) => o.copy_from_slice(&v.to_le_bytes()),
                        (false, 2) => o.copy_from_slice(&((v * 32767.0) as i16).to_le_bytes()),
                        (false, 4) => o.copy_from_slice(&((v as f64 * 2_147_483_647.0) as i32).to_le_bytes()),
                        _ => o.fill(0),
                    }
                }
            }
            render.ReleaseBuffer(n, 0).map_err(|e| e.to_string())
        };
        fill(size, &mut phase)?;
        client.Start().map_err(|e| e.to_string())?;
        let mut underruns = 0u32;
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
            let pad = client.GetCurrentPadding().map_err(|e| e.to_string())?;
            if pad == 0 {
                underruns += 1;
            }
            fill(size - pad, &mut phase)?;
        }
        let _ = client.Stop();
        Ok(underruns)
    }

    fn com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> std::thread::JoinHandle<T> {
        std::thread::spawn(move || unsafe {
            let ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let r = f();
            if ok {
                CoUninitialize();
            }
            r
        })
    }

    fn goxlr(capture: bool, name: &str) -> Option<Endpoint> {
        crate::audio::active_endpoints(capture).into_iter().find(|e| e.goxlr && e.name == name)
    }

    pub fn run(secs: f64) -> Value {
        let mut targets: Vec<(Endpoint, bool)> = vec![];
        for e in crate::audio::active_endpoints(true).into_iter().filter(|e| e.goxlr) {
            targets.push((e, false));
        }
        for e in crate::audio::active_endpoints(false).into_iter().filter(|e| e.goxlr) {
            targets.push((e, true));
        }
        let handles: Vec<_> = targets
            .into_iter()
            .map(|(e, lb)| {
                com(move || unsafe {
                    match listen(&e.id, lb, secs, &mut |_, _| {}) {
                        Ok(s) => verdict(&e.name, lb, secs, &s),
                        Err(err) => json!({"name": e.name, "kind": if lb { "output" } else { "input" }, "status": "error", "error": err}),
                    }
                })
            })
            .collect();
        let channels: Vec<Value> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
        json!({"type": "sound-check", "seconds": secs, "channels": channels})
    }

    pub fn loop_test(secs: f64, exclusive: bool) -> Value {
        let (Some(out), Some(inp)) = (goxlr(false, "Sample"), goxlr(true, "Broadcast Stream Mix")) else {
            return json!({"status": "unavailable", "error": "voies Sample ou Broadcast Stream Mix introuvables"});
        };
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let rec = com(move || unsafe {
            let mut blocks: Option<ToneBlocks> = None;
            let r = listen(&inp.id, false, secs, &mut |x, ch| {
                let t = blocks.get_or_insert_with(|| ToneBlocks::new(48_000.0));
                for fr in x.chunks_exact(ch.max(1) as usize) {
                    t.push(fr[0]);
                }
            });
            (r.map(|s| verdict(&inp.name, false, secs, &s)), blocks.map(|b| b.blocks).unwrap_or_default())
        });
        std::thread::sleep(Duration::from_millis(150));
        let play = com(move || unsafe { play_tone(&out.id, &s2, exclusive) });
        let (cap, blocks) = rec.join().unwrap_or((Err("écoute interrompue".into()), vec![]));
        stop.store(true, Ordering::SeqCst);
        let played = play.join().unwrap_or(Err("lecture interrompue".into()));
        match (cap, played) {
            (Ok(c), Ok(under)) => loop_verdict(&blocks, 48_000.0, under, &c),
            (Err(e), _) | (_, Err(e)) => json!({"status": "error", "error": e}),
        }
    }
}

pub fn run(secs: f64) -> Value {
    let secs = secs.clamp(3.0, 900.0);
    #[cfg(windows)]
    {
        win::run(secs)
    }
    #[cfg(not(windows))]
    {
        json!({"type": "sound-check", "seconds": secs, "channels": [], "unsupported": true})
    }
}

/// Test en boucle (le moteur doit avoir isolé la voie Sample : action « sound-test »).
/// \`exclusive\` (diagnostic) : le son de test va droit au pilote, sans le mixeur ni les effets de Windows.
pub fn loop_test(secs: f64, exclusive: bool) -> Value {
    let secs = secs.clamp(3.0, 900.0);
    #[cfg(windows)]
    {
        win::loop_test(secs, exclusive)
    }
    #[cfg(not(windows))]
    {
        let _ = exclusive;
        json!({"status": "unavailable", "seconds": secs, "unsupported": true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, fs: f64, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * LOOP_FREQ * i as f64 / fs).sin() as f32).collect()
    }

    #[test]
    fn boucle_propre() {
        let mut x = tone(48000 * 3, 48000.0, 0.1);
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.02 * ((i as f32 * 0.37).sin()); // un peu de voix grave par-dessus
        }
        let (n, amp, ev) = analyse_tone(&x, 48000.0);
        assert!(n > 1400);
        assert!((amp - 0.1).abs() < 0.01, "{amp}");
        assert!(ev.is_empty(), "{ev:?}");
    }

    #[test]
    fn boucle_paquet_perdu_et_trou() {
        let mut x = vec![0.0; 4800]; // le son arrive apres 100 ms : pas un trou
        x.extend(tone(48000 * 2, 48000.0, 0.1));
        x.drain(52800..52848); // 1 ms d'echantillons perdus : saut de phase
        for v in &mut x[74800..75040] {
            *v = 0.0; // 5 ms de silence
        }
        x.extend(vec![0.0; 4800]); // fin du test
        let (_, _, ev) = analyse_tone(&x, 48000.0);
        let kinds: Vec<&str> = ev.iter().map(|e| e.1).collect();
        assert_eq!(kinds, vec!["jump", "dropout"], "{ev:?}");
        assert!((ev[0].0 - 1.1).abs() < 0.01);
        let v = loop_verdict(&{
            let mut t = ToneBlocks::new(48000.0);
            x.iter().for_each(|s| t.push(*s));
            t.blocks
        }, 48000.0, 0, &json!({}));
        assert_eq!((v["status"].as_str(), v["dropouts"].as_u64(), v["jumps"].as_u64()), (Some("glitches"), Some(1), Some(1)));
        assert_eq!(v["level_db"], -20.0);
    }

    #[test]
    fn boucle_sans_son() {
        let (n, amp, ev) = analyse_tone(&vec![0.0; 48000], 48000.0);
        assert!(n == 0 && amp < 1e-4 && ev.is_empty());
        assert_eq!(loop_verdict(&[], 48000.0, 0, &json!({}))["status"], "no-signal");
    }

    #[test]
    fn trous_et_discontinuites() {
        let mut s = Stats { sample_rate: 48000, ..Default::default() };
        let quiet = vec![0.01f32; 960];
        s.add(480, 1, Some(0), None, &quiet, true); // premier paquet : drapeau ignoré
        s.add(480, 0, Some(480), Some(480), &quiet, true);
        s.add(480, 0, Some(1440), Some(960), &quiet, true); // 480 trames perdues
        s.add(480, 1, Some(1920), Some(1920), &quiet, true);
        assert_eq!((s.discontinuities, s.gaps, s.gap_frames), (1, 1, 480));
        let v = verdict("Chat Mic", false, 0.04, &s);
        assert_eq!(v["status"], "dropouts");
        assert_eq!(v["gap_ms"], 10.0);
    }

    #[test]
    fn saturation_en_sortie() {
        let mut s = Stats { sample_rate: 48000, ..Default::default() };
        let mut buf = vec![0.2f32; 960];
        buf[10] = 1.3;
        buf[11] = -1.0;
        buf[12] = 1.0;
        s.add(480, 0, Some(0), None, &buf, false);
        assert_eq!((s.clipped, s.overs), (3, 1));
        assert_eq!(s.peak_db(), Some(2.3));
        assert_eq!(verdict("Game", true, 0.01, &s)["status"], "clipping");
        let calm = Stats { sample_rate: 48000, packets: 10, frames: 480_000, peak: 0.5, sound_frames: 48000, ..Default::default() };
        assert_eq!(verdict("Game", true, 10.0, &calm)["status"], "ok");
    }

    #[test]
    fn entree_qui_livre_trop_peu() {
        let s = Stats { sample_rate: 48000, packets: 100, frames: 48000 * 8, ..Default::default() };
        assert_eq!(verdict("Chat Mic", false, 10.0, &s)["status"], "dropouts");
        let s = Stats { sample_rate: 48000, packets: 100, frames: 48000 * 10 - 960, ..Default::default() };
        assert_eq!(verdict("Chat Mic", false, 10.0, &s)["status"], "ok");
    }

    #[test]
    fn formats() {
        assert_eq!(to_f32(&0.5f32.to_le_bytes(), true, 32), vec![0.5]);
        assert_eq!(to_f32(&i16::MIN.to_le_bytes(), false, 16), vec![-1.0]);
        let v = to_f32(&[0, 0, 0x40], false, 24);
        assert!((v[0] - 0.5).abs() < 1e-6);
    }
}
