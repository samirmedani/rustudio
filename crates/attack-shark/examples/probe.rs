//! Diagnostic temporaire : lecture seule par defaut, aucun installateur.
#[cfg(windows)]
fn main() {
    let result = (|| -> rustudio_attack_shark::Result<serde_json::Value> {
        use rustudio_attack_shark::{Hid, Mouse, Transport, frame};
        if std::env::args().any(|a| a == "--read-reports") {
            let mut hid = Hid::open()?;

let mut out = Vec::new();
            for (command, body) in [(0x85, vec![0]), (0x83, vec![0,0]), (0x81, vec![])] {
                let q = frame(0, command, &body)?;
                out.push(serde_json::json!({"command":command,"response":hid.exchange(&q)}));
            }
            return Ok(serde_json::json!({"reports":out}));
        }
        let mut hid = Hid::open()?;
        let verify = std::env::args().any(|a| a == "--verify-write");
        let waiting = std::env::args().any(|a| a == "--wait");
        if verify && waiting {
            // Deux petites lectures au reveil, avant les longues lectures de configuration.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
            println!("{}", serde_json::json!({"phase":"waiting"}));
            let (profile, sleep) = loop {
                let ready = (|| -> rustudio_attack_shark::Result<(u8,u16)> {
                    let q = frame(0,0x85,&[0])?; let r = hid.exchange(&q)?;
                    let p = rustudio_attack_shark::payload(&r,&q)?[0];
                    if !(1..=4).contains(&p) { return Err("Profil inconnu".into()); }
                    let q = frame(0,0x87,&[p])?; let r = hid.exchange(&q)?;
                    let data = rustudio_attack_shark::payload(&r,&q)?;
                    if data.len() < 3 { return Err("Veille inconnue".into()); }
                    Ok((p,u16::from_be_bytes([data[1],data[2]])))
                })();
                match ready { Ok(v) => break v, Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_secs(1)), Err(e) => return Err(e) }
            };
            println!("{}", serde_json::json!({"phase":"awake","profile":profile,"original_sleep_seconds":sleep}));
            let hold = hid.exchange(&frame(0,7,&[profile,0,120])?);
            eprintln!("hold_ack: {:?}", hold.as_ref().map(|r| &r[..r.len().min(16)]));
            let mut baseline_saved: Option<rustudio_attack_shark::Settings> = None;
            let result = (|| -> rustudio_attack_shark::Result<serde_json::Value> {
                let mut mouse = Mouse::new(&mut hid);
                let mut before = mouse.read()?;
                if before.sleep_seconds != 120 { return Err("La prolongation de veille n'a pas été confirmée".into()); }
                if before.profile != profile { return Err("Le profil actif a changé pendant le test".into()); }
                before.sleep_seconds = sleep;
                baseline_saved = Some(before.clone());
                println!("{}", serde_json::json!({"phase":"testing","before":before}));
                let baseline = serde_json::to_value(&before).unwrap();
                let mut stages = serde_json::to_value(&before.stages).unwrap();
                let i = if before.active_stage == 1 && before.stages.len() > 1 { 1 } else { 0 };
                let old = before.stages[i].x;
                stages[i]["x"] = serde_json::json!(if old < before.dpi_max { old + 50 } else { old - 50 });
                for (key, value) in [("debounce_ms",serde_json::json!(if before.debounce_ms < 30 { before.debounce_ms + 1 } else { before.debounce_ms - 1 })), ("polling_hz",serde_json::json!(if before.polling_hz == 1000 {500} else {1000})), ("stages",stages)] {
                    let changed = mouse.apply(key,&value);
                    let restore = mouse.apply(key,&baseline[key]);
                    changed?; restore?;
                    println!("{}",serde_json::json!({"phase":"verified","setting":key}));
                }
                Ok(serde_json::json!({"before":before}))
            })();
            // Si la verification a echoue, les restaurations ne dependent plus d'une lecture complete.
            if result.is_err() {
                if let Some(s) = &baseline_saved {
                    let mut stages = vec![s.profile, s.stages.len() as u8];
                    for d in &s.stages { stages.extend(d.x.to_be_bytes()); stages.extend(d.y.to_be_bytes()); }
                    let rate = rustudio_attack_shark::RATES.iter().find(|(hz,_)| *hz == s.polling_hz).unwrap().1;
                    for (target,cmd,body) in [(0,8,vec![s.profile,s.debounce_ms]),(1,0,vec![s.profile,rate]),(1,1,stages)] {
                        let _ = hid.exchange(&frame(target,cmd,&body)?);
                    }
                }
            }
            // Restauration brute de la veille connue, meme si une lecture ou un test echoue.
            let bytes = sleep.to_be_bytes();
            let _ = hid.exchange(&frame(0,7,&[profile,bytes[0],bytes[1]])?);
            let restored = Mouse::new(&mut hid).read()?;
            println!("{}",serde_json::json!({"phase":"restored","state":restored}));
            let data = result?;
            let mut before: rustudio_attack_shark::Settings = serde_json::from_value(data["before"].clone()).unwrap();
            before.battery = restored.battery; before.charging = restored.charging;
            if before != restored { return Err("Etat final différent de la baseline".into()); }
            return Ok(serde_json::json!({"write_verified":true,"restored":restored}));
        }
        let mut mouse = Mouse::new(hid);
        let before = mouse.read()?;
        Ok(serde_json::json!({"state":before}))
    })();
    match result { Ok(v) => println!("{v}"), Err(e) => { eprintln!("{e}"); std::process::exit(1); } }
}
#[cfg(not(windows))]
fn main() { eprintln!("Le diagnostic du matériel réel nécessite Windows."); }
