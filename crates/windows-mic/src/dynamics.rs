//! Réglages entiers partagés entre le moteur, le calcul audio et les contrôles de la page.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dynamics {
    pub gate: bool,
    pub threshold: i32,
    pub depth: i32,
    pub hold_ms: i32,
    pub compressor: bool,
    pub comp_db: i32,
    pub ratio: i32,
    pub attack_ms: i32,
    pub release_ms: i32,
    pub makeup: i32,
    pub deesser: i32,
    pub limiter: bool,
    pub ceiling: i32,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            gate: false,
            threshold: -48,
            depth: 60,
            hold_ms: 80,
            compressor: false,
            comp_db: -18,
            ratio: 3,
            attack_ms: 8,
            release_ms: 100,
            makeup: 0,
            deesser: 0,
            limiter: false,
            ceiling: -1,
        }
    }
}

impl Dynamics {
    pub fn sanitize(&mut self) {
        self.threshold = self.threshold.clamp(-80, -15);
        self.depth = self.depth.clamp(6, 80);
        self.hold_ms = self.hold_ms.clamp(0, 500);
        self.comp_db = self.comp_db.clamp(-48, -3);
        self.ratio = self.ratio.clamp(1, 20);
        self.attack_ms = self.attack_ms.clamp(1, 80);
        self.release_ms = self.release_ms.clamp(10, 600);
        self.makeup = self.makeup.clamp(0, 12);
        self.deesser = self.deesser.clamp(0, 100);
        self.ceiling = self.ceiling.clamp(-12, 0);
    }

    pub fn updated(self, patch: &Value) -> Result<Self, String> {
        let mut fields = serde_json::to_value(self).map_err(|e| e.to_string())?;
        for (k, v) in patch
            .as_object()
            .ok_or("Réglages de dynamique illisibles.")?
        {
            let slot = fields.get_mut(k).ok_or("Réglage de dynamique inconnu.")?;
            *slot = v.clone();
        }
        let mut next: Self =
            serde_json::from_value(fields).map_err(|_| "Valeur de dynamique invalide.")?;
        next.sanitize();
        Ok(next)
    }

    pub fn preset(id: &str) -> Self {
        let mut s = Self::default();
        match id {
            "streamer" | "radio" | "clear" | "soft" => {
                s.gate = true;
                s.compressor = true;
                s.limiter = true;
                s.makeup = 3;
                if id == "radio" {
                    s.comp_db = -24;
                    s.ratio = 5;
                    s.makeup = 6;
                }
                if id == "soft" {
                    s.deesser = 45;
                    s.ratio = 2;
                }
                if id == "clear" {
                    s.deesser = 20;
                }
            }
            _ => {}
        }
        s
    }
}
