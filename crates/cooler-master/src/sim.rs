//! Faux MK770 : mémoire de configuration, tampon par touche et trames du câble, avec les pièges du vrai
//! clavier (102 LED relues pour 104 écrites, écriture partielle du tampon ignorée, rapports parasites).
use crate::*;

#[derive(Clone)]
pub struct Sim {
    pub memory: [u8; 805],
    pub keys: [[u8; 3]; data::LEDS],
    pub disconnected: bool,
    pub writes: usize,
    /// rapport parasite glissé avant chaque réponse (comme une réponse en retard)
    pub noise: bool,
    pub macro_map: [u16; 104],
    pub macro_records: Vec<Vec<u8>>,
    macro_incoming: Vec<u8>,
    macro_next: u8,
    pending: std::collections::VecDeque<Vec<u8>>,
    incoming: Vec<[u8; REPORT]>,
}

/// Fiches relevées sur un vrai MK770 (14 novembre 2026), effet 0 actif.
const RECORDS: [[u8; 10]; 20] = [
    [0xFF, 0xFF, 0x02, 0x8A, 0x2B, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0x07], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0xFF, 0x00, 0x00, 0, 0, 0, 0x00],
    [0xFF, 0xFF, 0x02, 0xFF, 0x50, 0x00, 0, 0, 0, 0x01], [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0xFF, 0xFF, 0x02, 0x00, 0x7F, 0xFF, 0, 0, 0, 0x04],
    [0xFF, 0xFF, 0x02, 0xFF, 0x00, 0x00, 0, 0, 0, 0x00], [0xFF, 0xFF, 0x02, 0xFF, 0x00, 0x00, 0, 0, 0, 0x00],
    [0xFF, 0xFF, 0x02, 0x8B, 0x00, 0xFF, 0, 0, 0, 0x06], [0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0, 0, 0, 0x06],
];

impl Default for Sim {
    fn default() -> Self {
        let mut memory = [0u8; 805];
        for (i, r) in RECORDS.iter().enumerate() {
            memory[i * 10..i * 10 + 10].copy_from_slice(r);
        }
        let mut keys = [[0u8; 3]; data::LEDS];
        // une rangée de chiffres colorée, comme une jauge laissée par un autre logiciel
        for (i, k) in keys.iter_mut().enumerate().skip(19).take(6) {
            *k = if i < 22 { [0x00, 0xFF, 0x00] } else { [0xFF, 0xFF, 0x00] };
        }
        let macro_records = (0..12).map(|id| { let mut b=vec![0;355]; b[0]=id; b }).collect();
        Self { memory, keys, disconnected: false, writes: 0, noise: false, macro_map: [0;104], macro_records, macro_incoming: vec![], macro_next:0, pending: Default::default(), incoming: vec![] }
    }
}

impl Sim {
    fn reply(&mut self, r: Vec<u8>) {
        if self.noise {
            let mut junk = vec![0u8; REPORT];
            junk[0] = 0x83;
            junk[1] = MARK;
            self.pending.push_back(junk);
        }
        let mut r = r;
        r.resize(REPORT, 0);
        self.pending.push_back(r);
    }
    fn keys_frame(&mut self, report: &[u8; REPORT]) {
        if report[1] & 0x80 != 0 {
            self.incoming = vec![*report];
            return;
        }
        if self.incoming.is_empty() || report[1] as usize != self.incoming.len() {
            self.incoming.clear(); // trame incomplète ou désordonnée : le vrai clavier ne l'affiche pas
            return;
        }
        self.incoming.push(*report);
        let count = (self.incoming[0][1] & 0x7F) as usize;
        if self.incoming.len() == count {
            let mut bytes: Vec<u8> = self.incoming[0][6..].to_vec();
            for f in &self.incoming[1..] {
                bytes.extend_from_slice(&f[2..]);
            }
            if bytes.len() >= KEYS_WRITE_BYTES {
                for (i, c) in bytes[..KEYS_WRITE_BYTES].chunks_exact(4).enumerate() {
                    self.keys[i] = [c[0], c[1], c[2]];
                }
                self.writes += 1;
            }
            self.incoming.clear();
        }
    }
}

impl Transport for Sim {
    fn send(&mut self, r: &[u8; REPORT]) -> Result<()> {
        if self.disconnected {
            return Err("Clavier simulé débranché".into());
        }
        if r[0] == OP_KEYS_WRITE {
            self.keys_frame(r);
            return Ok(());
        }
        match r[0] {
            4 => {
                if r[1] & 0x80 != 0 { self.macro_incoming.clear(); self.macro_next=1; if r[1]!=0x86{return Ok(());} }
                else if self.macro_incoming.is_empty()||r[1]!=self.macro_next {self.macro_incoming.clear();return Ok(());}
                else {self.macro_next+=1;}
                self.macro_incoming.extend_from_slice(&r[2..]);
                if self.macro_incoming.len() >= 355 {
                    let id=self.macro_incoming[0] as usize;
                    if id < 12 { self.macro_records[id].copy_from_slice(&self.macro_incoming[..355]); self.writes+=1; }
                    self.macro_incoming.clear();
                }
                return Ok(());
            }
            5 if r[2] < 12 => {
                for frame in crate::macros::frames(5, &self.macro_records[r[2] as usize]) { self.reply(frame.to_vec()); }
                return Ok(());
            }
            6 if r[2] < 12 => { let b=&mut self.macro_records[r[2] as usize]; b.fill(0); b[0]=r[2]; self.writes+=1; return Ok(()); }
            8 if r[2] < 104 && r[3] == 1 => { self.macro_map[r[2] as usize]=u16::from_le_bytes([r[4],r[5]]); self.writes+=1; return Ok(()); }
            9 => {
                let b:Vec<_>=self.macro_map.iter().flat_map(|v|v.to_le_bytes()).collect();
                for frame in crate::macros::frames(9,&b) { self.reply(frame.to_vec()); }
                return Ok(());
            }
            _ => {}
        }
        if r[1] != MARK {
            return Ok(()); // rapport ignoré, sans réponse, comme le vrai clavier
        }
        let address = u16::from_le_bytes([r[2], r[3]]) as usize;
        let len = u16::from_le_bytes([r[4], r[5]]) as usize;
        match r[0] {
            OP_READ if address + len <= self.memory.len() && len <= REPORT - 6 => {
                let mut out = vec![OP_READ, MARK, r[2], r[3], r[4], r[5]];
                out.extend_from_slice(&self.memory[address..address + len]);
                self.reply(out);
            }
            OP_WRITE if address + len <= self.memory.len() && len <= REPORT - 6 => {
                self.memory[address..address + len].copy_from_slice(&r[6..6 + len]);
                self.writes += 1;
            }
            OP_KEYS_READ => {
                let mut bytes = Vec::new();
                for k in self.keys.iter().take(data::LEDS_READ) {
                    bytes.extend_from_slice(&[k[0], k[1], k[2], 0xFF]);
                }
                let count = 1 + (bytes.len() - FIRST_DATA).div_ceil(NEXT_DATA);
                let mut first = vec![OP_KEYS_READ, 0x80 | count as u8, 0, 0, r[4], r[5]];
                first.extend_from_slice(&bytes[..FIRST_DATA]);
                self.reply(first);
                for (n, chunk) in bytes[FIRST_DATA..].chunks(NEXT_DATA).enumerate() {
                    let mut f = vec![OP_KEYS_READ, n as u8 + 1];
                    f.extend_from_slice(chunk);
                    self.reply(f);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn receive(&mut self, _timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        if self.disconnected {
            return Err("Clavier simulé débranché".into());
        }
        Ok(self.pending.pop_front())
    }
    fn wired(&self) -> bool {
        true
    }
    fn firmware(&self) -> Option<String> {
        Some("1.07".into())
    }
}
