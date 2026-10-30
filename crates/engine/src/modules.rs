//! Adaptateurs des modules compiles : vues et actions propres a chaque appareil.
//! Aucun acces direct au materiel : les vues relisent l'etat connu et les actions passent par son controleur.
use crate::api::{ApiError, ApiResult};
use crate::controller::{Ctl, Inner};
use rustudio_core::modules::{self, ModuleRuntime, ModuleStatus};
use serde_json::{json, Value};
use std::sync::Arc;

fn goxlr_runtime(health: &str, connected: bool, simulated: bool, firmware: Option<String>, error: Option<String>) -> ModuleRuntime {
    let status = match health {
        "ok" => ModuleStatus::Ready,
        "absent" => ModuleStatus::Disconnected,
        "starting" => ModuleStatus::Starting,
        "blocked" => ModuleStatus::Blocked,
        _ => ModuleStatus::Error,
    };
    ModuleRuntime {
        id: modules::GOXLR_MINI.id, status, connected, simulated,
        firmware: if connected { firmware } else { None },
        error: if status == ModuleStatus::Ready { None } else { error },
    }
}

impl Ctl {
    pub(crate) fn module_devices_locked(&self, g: &Inner, firmware: Option<String>) -> [ModuleRuntime; 2] {
        [goxlr_runtime(self.health_status(g), g.connected, self.simulated(), firmware, g.last_error.clone()), self.mouse.runtime()]
    }

    pub fn modules_snapshot(&self) -> Value {
        let firmware = self.dev_info.lock().unwrap().firmware.clone();
        let g = self.lock();
        json!({"installed": modules::installed(), "devices": self.module_devices_locked(&g, firmware)})
    }

    pub fn module_state_snapshot(&self, id: &str) -> ApiResult {
        let module = modules::find(id).ok_or_else(|| unknown_module(id))?;
        match module.id {
            "goxlr-mini" => {
                let firmware = self.dev_info.lock().unwrap().firmware.clone();
                let v = self.version();
                let g = self.lock();
                let live = &g.live;
                Ok(json!({"module": module, "device": self.module_devices_locked(&g, firmware)[0],
                    "state": g.state.to_value(), "meta": crate::views::meta(&g.state, g.connected && g.mix2),
                    "live": {"fader_positions": live.fader_positions, "mic_db": live.mic_db,
                        "pressed": live.pressed, "encoders": live.encoders}, "v": v}))
            }
            "attack-shark-r5-ultra" => {
                let data = self.mouse.state();
                Ok(json!({"module":module,"device":self.mouse.runtime(),"state":data["state"],"v":data["v"],"meta":{},"live":{}}))
            }
            _ => Err(unknown_module(id)),
        }
    }

    pub fn module_act(self: &Arc<Self>, id: &str, action: &str, p: &Value) -> ApiResult {
        let module = modules::find(id).ok_or_else(|| unknown_module(id))?;
        if !module.actions.contains(&action) {
            return Err(ApiError { status: 404, msg: format!("action inconnue pour ce module : {action}") });
        }
        match module.id {
            "goxlr-mini" if action == "echo-check" => Ok(crate::library::echo_check(self)),
            "goxlr-mini" => self.act(action, p),
            "attack-shark-r5-ultra" => self.mouse.command(action, p).map_err(|msg| ApiError { status: 400, msg }),
            _ => Err(unknown_module(id)),
        }
    }
}

fn unknown_module(id: &str) -> ApiError {
    ApiError { status: 404, msg: format!("module inconnu : {id}") }
}
