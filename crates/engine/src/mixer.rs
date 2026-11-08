//! Vue compacte du mixeur ; attend une modification de l'état en cache, jamais l'USB.
use crate::api::{ApiError, ApiResult};
use crate::controller::{Ctl, Inner};
use rustudio_core::consts::{
    CHANNEL_COLOURS, CHANNEL_LABELS, FADERS, IN_CHANNEL, MUTE_TARGET_LABELS,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Waiter<'a>(&'a AtomicUsize);
impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Ctl {
    fn mixer_cursor(&self, g: &Inner) -> String {
        // Les poignées peuvent bouger sans modifier le volume logiciel. 256 code None.
        let [a, b, c, d] = g
            .live
            .fader_positions
            .map(|p| p.map(u16::from).unwrap_or(256));
        format!(
            "{}-{}-{}-{a}-{b}-{c}-{d}",
            self.boot_id,
            g.version,
            self.health_status(g)
        )
    }

    pub fn mixer_snapshot(&self, after: Option<&str>) -> ApiResult {
        if after.is_some_and(|cursor| cursor.len() > 128) {
            return Err(ApiError::bad("curseur de mixeur invalide"));
        }
        let mut g = self.lock();
        if let Some(after) = after.filter(|&cursor| self.mixer_cursor(&g) == cursor) {
            // Le serveur a huit travailleurs. Les lectures suspendues ne peuvent jamais les
            // monopoliser : on conserve les autres pour les commandes, l'UI et le diagnostic.
            self.mixer_waiters
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < 2).then_some(n + 1)
                })
                .map_err(|_| ApiError {
                    status: 429,
                    msg: "trop de lecteurs du mixeur".into(),
                })?;
            let _slot = Waiter(&self.mixer_waiters);
            g = self
                .mixer_changed
                .wait_timeout_while(g, Duration::from_millis(750), |g| {
                    self.mixer_cursor(g) == after
                })
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let state = &g.state;
        let faders = FADERS.iter().enumerate().map(|(i, &id)| {
            json!({
                "id": id, "channel": state.fader_channel(id), "position": g.live.fader_positions[i],
                "mode": state.mute_modes.get(id).map(String::as_str).unwrap_or("all"),
            })
        }).collect::<Vec<_>>();
        let channels = CHANNEL_LABELS
            .iter()
            .zip(CHANNEL_COLOURS)
            .map(|(&(id, label), (_, colour))| {
                json!({
                    "id": id, "label": label, "colour": colour, "volume": state.volume(id),
                    "mute": if state.muted(id) { Some("all") } else { state.partial_of(id) },
                    "input": IN_CHANNEL.iter().any(|&(_, channel)| channel == id),
                })
            })
            .collect::<Vec<_>>();
        let targets = MUTE_TARGET_LABELS
            .iter()
            .map(|&(id, label)| json!({"id": id, "label": label}))
            .collect::<Vec<_>>();
        Ok(json!({
            "cursor": self.mixer_cursor(&g), "boot": self.boot_id, "connected": g.connected, "health": self.health_status(&g),
            "simulated": self.simulated(), "faders": faders, "channels": channels, "targets": targets,
        }))
    }
}
