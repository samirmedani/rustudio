//! Sonde de diagnostic (Windows) : compteurs internes du pilote Thesycon pour la GoXLR
//! (TUSBAUDIO_QueryDeviceStatistics et TUSBAUDIO_QueryDriverStatistics), lus sans rien envoyer a l'appareil.
//!
//!     usbstats <secondes> [periode_ms]
//!     usbstats check          (chemin du moteur : Dll + Tusb::stream_stats)
//!
//! Affiche l'heure (secondes Unix) et les champs u32 qui changent rarement : un compteur d'erreurs de flux saute
//! au moment d'une coupure du son, alors que les compteurs de paquets changent a chaque lecture.

#[cfg(not(windows))]
fn main() {
    eprintln!("usbstats : Windows seulement");
}

#[cfg(windows)]
fn main() {
    win::main()
}

#[cfg(windows)]
mod win {
    use libloading::Library;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    type FnVoid = unsafe extern "system" fn() -> u32;
    type FnOpen = unsafe extern "system" fn(u32, *mut u32) -> u32;
    type FnDev = unsafe extern "system" fn(u32, *mut u8, u32, u32) -> u32;
    type FnDrv = unsafe extern "system" fn(u32, *mut u8, u32) -> u32;
    type FnClose = unsafe extern "system" fn(u32) -> u32;

    const DEV_SIZE: usize = 0x554;
    const DRV_SIZE: usize = 0x297FD0;

    fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }

    fn words(b: &[u8]) -> Vec<u32> {
        b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }

    struct Watch {
        name: &'static str,
        prev: Vec<u32>,
        changes: Vec<u32>,
        samples: u32,
    }

    impl Watch {
        fn new(name: &'static str, first: Vec<u32>) -> Watch {
            let n = first.len();
            Watch { name, prev: first, changes: vec![0; n], samples: 0 }
        }
        /// Champs qui changent : tout pendant les 2 premieres lectures, puis seulement ceux qui changent dans moins
        /// d'un quart des lectures (les compteurs de paquets avancent a chaque fois).
        fn step(&mut self, cur: Vec<u32>, t: f64) {
            self.samples += 1;
            let mut out = Vec::new();
            for (i, (a, b)) in self.prev.iter().zip(cur.iter()).enumerate() {
                if a != b {
                    self.changes[i] += 1;
                    let rare = self.samples <= 2 || self.changes[i] * 4 < self.samples;
                    if rare && out.len() < 40 {
                        out.push(format!("0x{:x}:{}->{}", i * 4, a, b));
                    }
                }
            }
            if !out.is_empty() {
                println!("{t:.3} {} {}", self.name, out.join(" "));
            }
            self.prev = cur;
        }
        fn summary(&self) {
            let mut moving: Vec<String> = Vec::new();
            for (i, c) in self.changes.iter().enumerate() {
                if *c > 0 {
                    moving.push(format!("0x{:x}:{}x={}", i * 4, c, self.prev[i]));
                }
            }
            println!("RESUME {} lectures={} champs_changes={} {}", self.name, self.samples, moving.len(),
                moving.iter().take(300).cloned().collect::<Vec<_>>().join(" "));
        }
    }

    /// \`usbstats check\` : le chemin exact du moteur (Dll + Tusb::stream_stats), trois lectures a une seconde.
    fn check() {
        use rustudio_goxlr::Tusb;
        let mut d = match rustudio_goxlr::dll::Dll::load(&rustudio_goxlr::dll::default_path()) {
            Ok(d) => d,
            Err(e) => return println!("DLL : {e}"),
        };
        println!("enumerate=0x{:x} appareils={}", d.enumerate(), d.device_count());
        let Ok(h) = d.open_by_index(0) else { return println!("ouverture impossible") };
        println!("instance={:?}", d.instance_id(h));
        for _ in 0..3 {
            println!("{:.3} {:?}", now(), d.stream_stats(h));
            std::thread::sleep(Duration::from_secs(1));
        }
        d.close(h);
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("check") {
            return check();
        }
        let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(60.0);
        let period: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
        let path = rustudio_goxlr::dll::default_path();
        // SAFETY : DLL du pilote officiel, depuis son dossier d'installation ; signatures relevees dans la DLL 5.57.
        unsafe {
            let lib = Library::new(&path).expect("DLL du pilote introuvable");
            let enumerate = lib.get::<FnVoid>(b"TUSBAUDIO_EnumerateDevices").unwrap();
            let count = lib.get::<FnVoid>(b"TUSBAUDIO_GetDeviceCount").unwrap();
            let open = lib.get::<FnOpen>(b"TUSBAUDIO_OpenDeviceByIndex").unwrap();
            let close = lib.get::<FnClose>(b"TUSBAUDIO_CloseDevice").unwrap();
            let dev = lib.get::<FnDev>(b"TUSBAUDIO_QueryDeviceStatistics").unwrap();
            let drv = lib.get::<FnDrv>(b"TUSBAUDIO_QueryDriverStatistics").unwrap();
            let e = enumerate();
            let n = count();
            let mut h = 0u32;
            let o = open(0, &mut h);
            println!("enumerate=0x{e:x} appareils={n} open=0x{o:x} handle={h}");
            if o != 0 {
                return;
            }
            let mut db = vec![0u8; DEV_SIZE];
            let mut rb = vec![0u8; DRV_SIZE];
            let c1 = dev(h, db.as_mut_ptr(), DEV_SIZE as u32, 0);
            let c2 = drv(h, rb.as_mut_ptr(), DRV_SIZE as u32);
            println!("device_stats=0x{c1:x} driver_stats=0x{c2:x} debut={:.3}", now());
            if let Ok(dir) = std::env::var("USBSTATS_DUMP") {
                let _ = std::fs::write(format!("{dir}/dev.bin"), &db);
                let _ = std::fs::write(format!("{dir}/drv.bin"), &rb);
            }
            let name: String = db[0x10..0x90].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
            let first = words(&db);
            let nz: Vec<String> = first.iter().enumerate().filter(|(_, v)| **v != 0).map(|(i, v)| format!("0x{:x}={}", i * 4, v)).collect();
            println!("DEV nom='{name}' non_nuls {}", nz.join(" "));
            let rfirst = words(&rb);
            println!("DRV non_nuls={}", rfirst.iter().filter(|v| **v != 0).count());
            // USBSTATS_FIELDS=1ec,1f0,... : ces champs du bloc pilote, a chaque lecture, dans USBSTATS_CSV
            let fields: Vec<usize> = std::env::var("USBSTATS_FIELDS").unwrap_or_default().split(',')
                .filter_map(|s| usize::from_str_radix(s.trim(), 16).ok()).filter(|o| o + 4 <= DRV_SIZE).collect();
            if !fields.is_empty() {
                let csv = std::env::var("USBSTATS_CSV").unwrap_or_else(|_| "usbstats.csv".into());
                let mut out = String::from("t");
                for f in &fields {
                    out.push_str(&format!(",{f:x}"));
                }
                out.push('\n');
                let t0 = Instant::now();
                while t0.elapsed().as_secs_f64() < secs {
                    if drv(h, rb.as_mut_ptr(), DRV_SIZE as u32) == 0 {
                        out.push_str(&format!("{:.3}", now()));
                        for f in &fields {
                            out.push_str(&format!(",{}", u32::from_le_bytes([rb[*f], rb[f + 1], rb[f + 2], rb[f + 3]])));
                        }
                        out.push('\n');
                    }
                    std::thread::sleep(Duration::from_millis(period));
                }
                let _ = std::fs::write(&csv, out);
                println!("CSV {csv}");
                close(h);
                return;
            }
            let mut wd = Watch::new("DEV", first);
            let mut wr = Watch::new("DRV", rfirst);
            let t0 = Instant::now();
            let mut k = 0u64;
            while t0.elapsed().as_secs_f64() < secs {
                std::thread::sleep(Duration::from_millis(period));
                k += 1;
                let t = now();
                if c1 == 0 && dev(h, db.as_mut_ptr(), DEV_SIZE as u32, 0) == 0 {
                    wd.step(words(&db), t);
                }
                // le gros bloc du pilote : une lecture par seconde
                if c2 == 0 && k % (1000 / period.max(1)).max(1) == 0 && drv(h, rb.as_mut_ptr(), DRV_SIZE as u32) == 0 {
                    wr.step(words(&rb), t);
                }
            }
            wd.summary();
            wr.summary();
            close(h);
        }
    }
}
