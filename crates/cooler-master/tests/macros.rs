use rustudio_cooler_master::{Keyboard, REPORT, Transport, sim::Sim};
use rustudio_core::keyboard_macro::{Assignment, Event, Macro, Mode};
fn assignment() -> Assignment {
    Assignment {
        slot: 2,
        key: 69,
        data: Macro {
            name: "Exemple".into(),
            mode: Mode::Once,
            events: vec![
                Event {
                    key: 224,
                    down: true,
                    delay_ms: 0,
                },
                Event {
                    key: 6,
                    down: true,
                    delay_ms: 10,
                },
                Event {
                    key: 6,
                    down: false,
                    delay_ms: 25,
                },
                Event {
                    key: 224,
                    down: false,
                    delay_ms: 5,
                },
            ],
        },
    }
}
#[test]
fn native_frames_and_independent_mapping_preserved() {
    let a = assignment();
    let f = rustudio_cooler_master::macros::frames(4, &a.data.encode(2).unwrap());
    assert_eq!(f.len(), 6);
    assert_eq!(&f[0][..5], &[4, 0x86, 2, 1, 4]);
    assert_eq!(&f[1][..2], &[4, 1]);
    assert_eq!(&f[5][..2], &[4, 5]);
    let mut sim = Sim::default();
    sim.noise = true;
    sim.macro_map[55] = 0x2005;
    let lights = sim.memory;
    let colours = sim.keys;
    {
        let mut kb = Keyboard::new(&mut sim);
        let slots = kb.assign_macro(&a).unwrap();
        assert_eq!(slots[0].id, 2);
        assert_eq!(slots[0].data, a.data);
        assert_eq!(slots[0].bindings[0].key, 69);
    }
    assert_eq!(sim.macro_map[12], 0x3002);
    assert_eq!(sim.macro_map[55], 0x2005);
    assert_eq!(sim.memory, lights);
    assert_eq!(sim.keys, colours);
    // Nouvelle instance : les macros restent dans l'appareil, sans état de l'application.
    assert_eq!(
        Keyboard::new(&mut sim).read_macros().unwrap()[0].data,
        a.data
    );
    let mut moved = a;
    moved.key = 68;
    moved.data.mode = Mode::Hold;
    Keyboard::new(&mut sim).assign_macro(&moved).unwrap();
    assert_eq!(sim.macro_map[12], 0);
    assert_eq!(sim.macro_map[11], 0x4002);
    assert!(Keyboard::new(&mut sim).remove_macro(2).unwrap().is_empty());
    assert_eq!(sim.macro_map[11], 0);
    assert_eq!(sim.macro_map[55], 0x2005);
}
#[test]
fn invalid_requests_never_write() {
    let mut sim = Sim::default();
    let mut a = assignment();
    a.data.events.pop();
    assert!(Keyboard::new(&mut sim).assign_macro(&a).is_err());
    assert_eq!(sim.writes, 0);
    let a = assignment();
    sim.macro_map[12] = 0x2004;
    assert!(Keyboard::new(&mut sim).assign_macro(&a).is_err());
    assert_eq!(sim.writes, 0);
}
struct Corrupt {
    sim: Sim,
    corrupt: bool,
}
impl Transport for Corrupt {
    fn send(&mut self, r: &[u8; REPORT]) -> Result<(), String> {
        self.sim.send(r)?;
        if r[0] == 4 && r[1] == 5 && self.corrupt {
            self.sim.macro_records[2][4] ^= 1;
            self.corrupt = false;
        }
        Ok(())
    }
    fn receive(&mut self, t: i32) -> Result<Option<Vec<u8>>, String> {
        self.sim.receive(t)
    }
    fn wired(&self) -> bool {
        true
    }
}
#[test]
fn failed_readback_restores_previous_record_and_binding() {
    let mut h = Corrupt {
        sim: Sim::default(),
        corrupt: true,
    };
    let before = h.sim.macro_records[2].clone();
    let e = Keyboard::new(&mut h)
        .assign_macro(&assignment())
        .unwrap_err();
    assert!(e.contains("restaurés"), "{e}");
    assert_eq!(h.sim.macro_records[2], before);
    assert_eq!(h.sim.macro_map, [0; 104]);
}

struct ProfileChange {
    sim: Sim,
    changed: bool,
}
impl Transport for ProfileChange {
    fn send(&mut self, r: &[u8; REPORT]) -> Result<(), String> {
        self.sim.send(r)?;
        if r[0] == 4 && r[1] == 5 && !self.changed {
            self.changed = true;
            self.sim.memory[804] = 1;
            self.sim.macro_map[55] = 0x2004;
        }
        Ok(())
    }
    fn receive(&mut self, t: i32) -> Result<Option<Vec<u8>>, String> {
        self.sim.receive(t)
    }
    fn wired(&self) -> bool {
        true
    }
}
#[test]
fn never_restore_into_another_profile() {
    let mut h = ProfileChange {
        sim: Sim::default(),
        changed: false,
    };
    let error = Keyboard::new(&mut h)
        .assign_macro(&assignment())
        .unwrap_err();
    assert!(error.contains("Restauration non confirmée"), "{error}");
    assert_eq!(h.sim.macro_map[55], 0x2004);
    assert_eq!(h.sim.macro_map[12], 0);
    assert_eq!(
        h.sim.writes, 1,
        "aucune écriture après le changement de profil"
    );
}

struct Unordered(Sim);
impl Transport for Unordered {
    fn send(&mut self, r: &[u8; REPORT]) -> Result<(), String> {
        self.0.send(r)
    }
    fn receive(&mut self, t: i32) -> Result<Option<Vec<u8>>, String> {
        let mut r = self.0.receive(t)?;
        if let Some(r) = r.as_mut() {
            if r[0] == 9 && r[1] == 1 {
                r[1] = 2;
            }
        }
        Ok(r)
    }
    fn wired(&self) -> bool {
        true
    }
}
#[test]
fn unordered_reply_never_permits_programming() {
    let mut h = Unordered(Sim::default());
    let error = Keyboard::new(&mut h)
        .assign_macro(&assignment())
        .unwrap_err();
    assert!(error.contains("désordonnée"), "{error}");
    assert_eq!(h.0.writes, 0);
}

struct IgnoreRemoval {
    sim: Sim,
    once: bool,
}
impl Transport for IgnoreRemoval {
    fn send(&mut self, r: &[u8; REPORT]) -> Result<(), String> {
        if self.once && r[0] == 8 && r[4..6] == [0, 0] {
            self.once = false;
            return Ok(());
        }
        self.sim.send(r)
    }
    fn receive(&mut self, t: i32) -> Result<Option<Vec<u8>>, String> {
        self.sim.receive(t)
    }
    fn wired(&self) -> bool {
        true
    }
}
#[test]
fn failed_removal_restores_record_and_assignment() {
    let mut sim = Sim::default();
    Keyboard::new(&mut sim).assign_macro(&assignment()).unwrap();
    let previous = sim.macro_records.clone();
    let map = sim.macro_map;
    let mut h = IgnoreRemoval { sim, once: true };
    let error = Keyboard::new(&mut h).remove_macro(2).unwrap_err();
    assert!(error.contains("restaurés"), "{error}");
    assert_eq!(h.sim.macro_records, previous);
    assert_eq!(h.sim.macro_map, map);
}
