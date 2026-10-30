use rustudio_attack_shark::{frame, payload, sim::Sim, Mouse, Transport};
use serde_json::json;

#[test]
fn etat_et_commandes_confirmees_par_relecture() {
    let mut mouse = Mouse::new(Sim::default());
    assert_eq!(mouse.read().unwrap().stages[1].x, 1600);
    for (key, value) in [("polling_hz", json!(8000)), ("active_stage", json!(3)), ("debounce_ms", json!(7)),
        ("sleep_seconds", json!(65535)), ("lod_mm", json!(0.7)), ("angle_snap", json!(true)),
        ("motion_sync", json!(false)), ("ripple_control", json!(true)),
        ("stages", json!([{"x":400,"y":800},{"x":1600,"y":1600},{"x":3200,"y":3200}]))] {
        let after = mouse.apply(key, &value).unwrap();
        assert_eq!(serde_json::to_value(after).unwrap()[key], value);
    }
}
#[test]
fn valeurs_dangereuses_et_commandes_non_documentees_refusees() {
    let mut mouse = Mouse::new(Sim::default());
    let before = mouse.read().unwrap();
    for (key, value) in [("firmware", json!(1)), ("debounce_ms", json!(256)), ("polling_hz", json!(9000)),
        ("active_stage", json!(0)), ("active_stage", json!(4)), ("lod_mm", json!(0.8)),
        ("sleep_seconds", json!(-1)), ("angle_snap", json!(1)), ("stages", json!([])),
        ("stages", json!([{"x":42001,"y":100}]))] {
        assert!(mouse.apply(key, &value).is_err(), "{key}");
        assert_eq!(mouse.read().unwrap(), before);
    }
    let mut wired = Sim::default(); wired.state.wired = true;
    assert!(Mouse::new(wired).apply("polling_hz", &json!(8000)).is_err());
}
#[test]
fn enveloppes_tronquees_ou_anciennes_reponses_refusees() {
    let mut sim = Sim::default(); let q = frame(0, 0x83, &[0,0]).unwrap();
    let r = sim.exchange(&q).unwrap();
    assert_eq!(payload(&r, &q).unwrap(), &[0,84]);
    assert_eq!(payload(&r[1..], &q).unwrap(), &[0,84]);
    for len in 0..9 { assert!(payload(&r[..len], &q).is_err()); }
    for i in [1,3,5,6] { let mut bad = r.clone(); bad[i] ^= 1; assert!(payload(&bad, &q).is_err()); }
    assert!(frame(0,0,&[0;59]).is_err());
}
#[test]
fn debranchement_et_ecriture_ignoree_ne_donnent_pas_de_faux_succes() {
    let mut sim = Sim::default(); sim.disconnected = true;
    assert!(Mouse::new(sim).read().is_err());
    struct Ignore(Sim);
    impl Transport for Ignore {
        fn wired(&self) -> bool {false}
        fn exchange(&mut self, b: &[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            let old = self.0.state.clone(); let r = self.0.exchange(b); self.0.state = old; r
        }
    }
    assert!(Mouse::new(Ignore(Sim::default())).apply("debounce_ms", &json!(7)).is_err());
}

#[test]
fn trame_capturee_sur_r5_ultra_independante_du_simulateur() {
    // Capture documentee dans miyoi/docs/PROTOCOL.md (MIT), trois paliers 1200/2400/3200.
    struct Captured(Sim);
    impl Transport for Captured {
        fn wired(&self) -> bool { false }
        fn exchange(&mut self, b: &[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            if (b[4],b[5]) == (1,0x81) {
                Ok(vec![0xa1,0,2,14,1,0x81,1,3,0x04,0xb0,0x04,0xb0,0x09,0x60,0x09,0x60,0x0c,0x80,0x0c,0x80])
            } else { self.0.exchange(b) }
        }
    }
    let state = Mouse::new(Captured(Sim::default())).read().unwrap();
    assert_eq!(state.stages.iter().map(|s| (s.x,s.y)).collect::<Vec<_>>(), [(1200,1200),(2400,2400),(3200,3200)]);
    let q = frame(0,0x83,&[0,0]).unwrap();
    // L'accuse du recepteur present sans reponse de la souris n'est pas une batterie a zero.
    assert!(payload(&[0xa0,0,2,2,0,0x83,0,0], &q).is_err());
}

#[test]
fn palier_overclocke_existant_preserve_malgre_limite_native_inferieure() {
    // R5 reelle : limite native 30000, dernier palier deja a 42000.
    let mut sim = Sim::default(); sim.state.stages.push(rustudio_attack_shark::Dpi { x: 42000, y: 42000 });
    let mut mouse = Mouse::new(sim); let before = mouse.read().unwrap();
    assert_eq!(before.dpi_max, 42000);
    let mut stages = serde_json::to_value(&before.stages).unwrap(); stages[0]["x"] = json!(850);
    let after = mouse.apply("stages", &stages).unwrap();
    assert_eq!(after.stages[0].x, 850); assert_eq!(after.stages[3].x, 42000);
}

#[test]
fn accuse_perdu_et_reglage_confirme_par_relecture() {
    struct LostAck(Sim);
    impl Transport for LostAck {
        fn wired(&self) -> bool { false }
        fn exchange(&mut self, b: &[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            let result = self.0.exchange(b);
            if b[5] < 0x80 { return Err("Accusé perdu après écriture".into()); }
            result
        }
    }
    let after = Mouse::new(LostAck(Sim::default())).apply("debounce_ms", &json!(5)).unwrap();
    assert_eq!(after.debounce_ms, 5);
}

#[test]
fn expiration_pendant_la_lecture_empeche_une_ecriture_tardive() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    use std::time::{Instant, Duration};
    struct Slow { sim: Sim, writes: Arc<AtomicUsize> }
    impl Transport for Slow {
        fn wired(&self) -> bool { false }
        fn exchange(&mut self, b: &[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            if b[5] < 0x80 { self.writes.fetch_add(1,Ordering::SeqCst); }
            // Derniere lecture avant l'ecriture : le delai expire pendant la reponse.
            if b[5] == 0x8a { std::thread::sleep(Duration::from_millis(100)); }
            self.sim.exchange(b)
        }
    }
    let writes = Arc::new(AtomicUsize::new(0));
    let mut mouse = Mouse::new(Slow { sim: Sim::default(), writes: writes.clone() });
    mouse.set_deadline(Some(Instant::now() + Duration::from_millis(50)));
    assert!(mouse.apply("debounce_ms", &json!(7)).is_err());
    assert_eq!(writes.load(Ordering::SeqCst),0);
}

#[test]
fn groupe_confirme_seulement_les_champs_modifies() {
    struct Count { sim: Sim, queries: Vec<(u8,u8)> }
    impl Transport for Count {
        fn wired(&self) -> bool {false}
        fn exchange(&mut self, b: &[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            self.queries.push((b[4],b[5])); self.sim.exchange(b)
        }
    }
    let mut transport = Count { sim:Sim::default(), queries:vec![] };
    let cached = transport.sim.state.clone();
    let result = Mouse::new(&mut transport).apply_changes(&cached,1,&json!({"debounce_ms":7,"polling_hz":2000,"motion_sync":false})).unwrap();
    assert_eq!((result.debounce_ms,result.polling_hz,result.motion_sync),(7,2000,false));
    assert_eq!(transport.sim.writes,3);
    assert_eq!(transport.queries.len(),8,"deux lectures de profil et une écriture/relecture par champ, aucune lecture complète");
    assert!(!transport.queries.contains(&(0,0x81)),"firmware non relu pour enregistrer un réglage");
    assert!(!transport.queries.contains(&(0,0x83)),"batterie non relue pour enregistrer un réglage");
}

#[test]
fn groupe_invalide_ou_profil_modifie_necrit_rien() {
    let mut sim = Sim::default(); let cached = sim.state.clone();
    for changes in [json!({"debounce_ms":7,"firmware":1}),json!({"angle_snap":true,"polling_hz":9000})] {
        assert!(Mouse::new(&mut sim).apply_changes(&cached,1,&changes).is_err());
        assert_eq!(sim.writes,0);
    }
    sim.state.profile = 2;
    assert!(Mouse::new(&mut sim).apply_changes(&cached,1,&json!({"debounce_ms":7})).is_err());
    assert_eq!(sim.writes,0);
}

#[test]
fn groupe_lod_entier_et_reduction_des_paliers() {
    let mut sim = Sim::default(); let cached = sim.state.clone();
    let result = Mouse::new(&mut sim).apply_changes(&cached,1,&json!({"lod_mm":1,"active_stage":1,"stages":[{"x":850,"y":900}]})).unwrap();
    assert_eq!(result.lod_mm,1.0); assert_eq!(result.active_stage,1);
    assert_eq!(result.stages[0].x,850);
}

#[test]
fn groupe_interrompu_necrit_pas_les_champs_suivants() {
    struct Ignore { sim:Sim, writes:usize }
    impl Transport for Ignore {
        fn wired(&self) -> bool {false}
        fn exchange(&mut self,b:&[u8;64]) -> rustudio_attack_shark::Result<Vec<u8>> {
            if b[5]<0x80 { self.writes+=1; let old=self.sim.state.clone();let r=self.sim.exchange(b);self.sim.state=old;r }
            else {self.sim.exchange(b)}
        }
    }
    let mut t=Ignore{sim:Sim::default(),writes:0};let cached=t.sim.state.clone();
    assert!(Mouse::new(&mut t).apply_changes(&cached,1,&json!({"angle_snap":true,"debounce_ms":7})).is_err());
    assert_eq!(t.writes,1,"arrêt au premier réglage non confirmé, pas de retransmission");
}

#[test]
fn paliers_modifies_exterieurement_ne_sont_pas_ecrases() {
    let mut sim=Sim::default();let cached=sim.state.clone();sim.state.stages[1].x=2400;
    assert!(Mouse::new(&mut sim).apply_changes(&cached,1,&json!({"stages":[{"x":850,"y":800},{"x":1600,"y":1600},{"x":3200,"y":3200}]})).is_err());
    assert_eq!(sim.writes,0);
}

#[test]
fn expiration_dun_groupe_empeche_les_ecritures_suivantes() {
    use std::time::{Instant,Duration};
    struct Slow(Sim);
    impl Transport for Slow {
        fn wired(&self)->bool {false}
        fn exchange(&mut self,b:&[u8;64])->rustudio_attack_shark::Result<Vec<u8>> {
            if (b[4],b[5])==(1,0x84) {std::thread::sleep(Duration::from_millis(80));}
            self.0.exchange(b)
        }
    }
    let mut t=Slow(Sim::default());let cached=t.0.state.clone();
    let mut mouse=Mouse::new(&mut t);mouse.set_deadline(Some(Instant::now()+Duration::from_millis(40)));
    assert!(mouse.apply_changes(&cached,1,&json!({"angle_snap":true,"debounce_ms":7})).is_err());
    assert_eq!(t.0.writes,1);
}
