//! Protocole local versionné. Entiers de taille fixe, aucun pointeur dans la vue partagée.
use crate::{Dynamics, Voice};
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering::SeqCst};

pub const WORDS: usize = 24;
pub const CLASS: &str = "{F032FD61-4408-4491-AD71-E802D82B33A3}";
pub const MAGIC: u32 = 0x314d5352;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls(pub [i32; WORDS]);

impl Controls {
    pub fn new(v: &Voice, noise: bool, strength: i32, grace: i32) -> Self {
        let d = v.dynamics;
        Self([
            v.enabled as i32,
            v.gain_db,
            if v.hpf { v.hpf_hz } else { 0 },
            v.eq[0],
            v.eq[1],
            v.eq[2],
            v.eq[3],
            v.eq[4],
            d.gate as i32,
            d.threshold,
            d.depth,
            d.hold_ms,
            d.compressor as i32,
            d.comp_db,
            d.ratio,
            d.attack_ms,
            d.release_ms,
            d.makeup,
            d.deesser,
            d.limiter as i32,
            d.ceiling,
            noise as i32,
            strength,
            grace,
        ])
    }

    pub fn dynamics(self) -> Dynamics {
        let a = self.0;
        let mut d = Dynamics {
            gate: a[8] != 0,
            threshold: a[9],
            depth: a[10],
            hold_ms: a[11],
            compressor: a[12] != 0,
            comp_db: a[13],
            ratio: a[14],
            attack_ms: a[15],
            release_ms: a[16],
            makeup: a[17],
            deesser: a[18],
            limiter: a[19] != 0,
            ceiling: a[20],
        };
        d.sanitize();
        d
    }
}

/// Un écrivain de réglages (le moteur, sérialisé), plusieurs lecteurs. Une publication interrompue
/// laisse la génération impaire. Le lecteur garde le dernier bloc cohérent, sans tourner en boucle.
#[derive(Default)]
#[repr(C)]
pub struct Shared {
    pub magic: AtomicU32,
    generation: AtomicU32,
    controls: [AtomicI32; WORDS],
    pub callbacks: AtomicU32,
    pub faults: AtomicU32,
    pub levels: [AtomicI32; 5],
}

impl Shared {
    pub fn publish(&self, c: Controls) {
        let even = self.generation.load(SeqCst) & !1;
        self.generation.store(even.wrapping_add(1), SeqCst);
        for (slot, value) in self.controls.iter().zip(c.0) {
            slot.store(value, SeqCst);
        }
        self.generation.store(even.wrapping_add(2), SeqCst);
        self.magic.store(MAGIC, SeqCst);
    }

    pub fn changed(&self, previous: Option<u32>) -> Option<(u32, Controls)> {
        let g = self.generation.load(SeqCst);
        if Some(g) == previous || g & 1 != 0 || self.magic.load(SeqCst) != MAGIC {
            return None;
        }
        let values = Controls(std::array::from_fn(|i| self.controls[i].load(SeqCst)));
        (self.generation.load(SeqCst) == g).then_some((g, values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aucune_lecture_partielle_pendant_les_publications() {
        let shared = Shared::default();
        std::thread::scope(|s| {
            let writer = s.spawn(|| {
                for value in 1..20000 {
                    shared.publish(Controls([value; WORDS]));
                }
            });
            while !writer.is_finished() {
                if let Some((_, c)) = shared.changed(None) {
                    assert!(c.0.iter().all(|v| *v == c.0[0]));
                }
            }
        });
        assert_eq!(shared.changed(None).unwrap().1 .0, [19999; WORDS]);
    }
}
