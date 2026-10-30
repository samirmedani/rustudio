//! Fausse R5 Ultra : memes enveloppes et relectures que le materiel.
use crate::*;
#[derive(Clone)]
pub struct Sim {
    pub state: Settings,
    pub writes: usize,
    pub disconnected: bool,
}
impl Default for Sim {
    fn default() -> Self {
        Self { state: Settings { firmware: "0.12.0.0".into(), wired: false, profile: 1, battery: 84, charging: false,
            stages: vec![Dpi { x: 800, y: 800 }, Dpi { x: 1600, y: 1600 }, Dpi { x: 3200, y: 3200 }],
            active_stage: 2, dpi_max: MAX_DPI, polling_hz: 1000, lod_mm: 1.0, debounce_ms: 4,
            sleep_seconds: 120, angle_snap: false, motion_sync: true, ripple_control: false }, writes: 0, disconnected: false }
    }
}
impl Transport for Sim {
    fn wired(&self) -> bool { self.state.wired }
    fn exchange(&mut self, b: &[u8; 64]) -> Result<Vec<u8>> {
        if self.disconnected { return Err("Souris simulée débranchée".into()); }
        let (target, command) = (b[4], b[5]);
        if b[2] != 2 || b[3] > 58 { return Err("Trame invalide".into()); }
        let d = &b[6..6 + b[3] as usize];
        if command < 0x80 {
            if d.first() != Some(&self.state.profile) { return Err("Profil invalide".into()); }
            match (target, command) {
                (1, 1) if d.len() >= 2 && d.len() == 2 + 4 * d[1] as usize => self.state.stages = d[2..].chunks_exact(4).map(|v| Dpi { x: u16::from_be_bytes([v[0], v[1]]), y: u16::from_be_bytes([v[2], v[3]]) }).collect(),
                (1, 2) if d.len() == 2 => self.state.active_stage = d[1],
                (1, 0) if d.len() == 2 => self.state.polling_hz = RATES.iter().find(|(_, v)| *v == d[1]).ok_or("Taux invalide")?.0,
                (1, 8) if d.len() == 2 => self.state.lod_mm = if d[1] == 0x87 { 0.7 } else { d[1] as f64 },
                (0, 8) if d.len() == 2 => self.state.debounce_ms = d[1],
                (0, 7) if d.len() == 3 => self.state.sleep_seconds = u16::from_be_bytes([d[1], d[2]]),
                (1, 4) if d.len() == 2 => self.state.angle_snap = d[1] == 1,
                (1, 9) if d.len() == 2 => self.state.motion_sync = d[1] == 1,
                (1, 10) if d.len() == 2 => self.state.ripple_control = d[1] == 1,
                _ => return Err("Commande non simulée".into()),
            }
            self.writes += 1;
        }
        let s = &self.state;
        let data = if command < 0x80 { d.to_vec() } else { match (target, command) {
            (0, 0x81) => vec![0, 12, 0, 0],
            (0, 0x85) => vec![s.profile],
            (0, 0x83) => vec![s.charging as u8, s.battery],
            (1, 0x8c) => 30000u16.to_be_bytes().to_vec(),
            (1, 0x81) => { let mut out = vec![s.profile, s.stages.len() as u8]; for v in &s.stages { out.extend(v.x.to_be_bytes()); out.extend(v.y.to_be_bytes()); } out },
            (1, 0x82) => vec![s.profile, s.active_stage],
            (1, 0x80) => vec![s.profile, RATES.iter().find(|(hz, _)| *hz == s.polling_hz).unwrap().1],
            (1, 0x88) => vec![s.profile, if s.lod_mm == 0.7 { 0x87 } else { s.lod_mm as u8 }],
            (0, 0x88) => vec![s.profile, s.debounce_ms],
            (0, 0x87) => { let v = s.sleep_seconds.to_be_bytes(); vec![s.profile, v[0], v[1]] },
            (1, 0x84) => vec![s.profile, s.angle_snap as u8],
            (1, 0x89) => vec![s.profile, s.motion_sync as u8],
            (1, 0x8a) => vec![s.profile, s.ripple_control as u8],
            _ => return Err("Lecture non simulée".into()),
        }};
        let mut r = vec![0; 65]; r[1] = 0xa1; r[3] = 2; r[4] = data.len() as u8; r[5] = target; r[6] = command;
        r[7..7 + data.len()].copy_from_slice(&data);
        Ok(r)
    }
}
