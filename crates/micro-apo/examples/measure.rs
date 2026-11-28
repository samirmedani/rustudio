//! Temps réel et mémoire du calcul natif, sans lecture, écriture ni périphérique audio.
use rustudio_micro_apo::stream::Stream;
use rustudio_windows_mic::{control::Controls, Voice};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};
static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Meter;
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            BYTES.fetch_add(l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        BYTES.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOC: Meter = Meter;
fn main() {
    let mut v = Voice {
        enabled: true,
        ..Default::default()
    };
    v.apply_preset("radio").unwrap();
    for noise in [false, true] {
        let c = Controls::new(&v, noise, 0, 200);
        // Initialisation des tables immuables du modèle avant de compter la vie d'un flux.
        drop(Stream::new(48000, 2, c).unwrap());
        let baseline = BYTES.load(Relaxed);
        let mut s = Stream::new(48000, 2, c).unwrap();
        let bytes = BYTES.load(Relaxed) - baseline;
        let mut input = [0f32; 960];
        for (i, x) in input.iter_mut().enumerate() {
            *x = 0.5 * (0.037 * i as f32).sin() + 0.15 * (0.23 * i as f32).sin();
        }
        let mut times = Vec::with_capacity(6000);
        let start = Instant::now();
        for _ in 0..6000 {
            let mut block = input;
            let timer = Instant::now();
            s.process(std::hint::black_box(&mut block));
            times.push(timer.elapsed().as_micros());
        }
        let elapsed = start.elapsed();
        times.sort_unstable();
        let (p99, max) = (times[5939], times[5999]);
        let delay = s.latency();
        drop(s);
        drop(times);
        let after = BYTES.load(Relaxed);
        assert_eq!(
            after, baseline,
            "mémoire Rust restant après fermeture du flux"
        );
        println!("48 kHz stereo, AI={noise}: 60 s audio, {:.3} s murales ({:.2}% d'un coeur en temps mural), p99={p99} us, max={max} us / 10000 us, {bytes} octets par flux, retard={delay} trames, delta liberation=0",elapsed.as_secs_f64(), elapsed.as_secs_f64()*100./60.);
    }
}
