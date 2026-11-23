//! Adaptateurs des modules compiles : vues et actions propres a chaque appareil.
//! Aucun acces direct au materiel : les vues relisent l'etat connu et les actions passent par son controleur.
use crate::api::{ApiError, ApiResult};
use crate::controller::{Ctl, Inner};
use rustudio_core::modules::{self, FirmwareKind, ModuleRuntime, ModuleStatus};
use rustudio_goxlr::DevInfo;
use serde_json::{json, Value};
use std::sync::Arc;

fn goxlr_runtime(health: &str, g: &Inner, simulated: bool, info: &DevInfo) -> ModuleRuntime {
    let status = match health {
        "ok" => ModuleStatus::Ready,
        "audio" => ModuleStatus::Audio,
        "absent" => ModuleStatus::Disconnected,
        "starting" => ModuleStatus::Starting,
        "blocked" => ModuleStatus::Blocked,
        _ => ModuleStatus::Error,
    };
    ModuleRuntime {
        id: modules::GOXLR_MINI.id, status, connected: g.connected, simulated,
        firmware: if g.connected { info.firmware.clone() } else { None },
        firmware_kind: goxlr_firmware_kind(g, info),
        error: if matches!(status, ModuleStatus::Ready | ModuleStatus::Audio) { None } else { g.last_error.clone() },
    }
}

/// Officiel ou Golden d'après la version lue à la connexion. Le firmware de laboratoire ne parle pas au pilote
/// GoXLR : seule la liste USB de Windows le voit.
pub(crate) fn goxlr_firmware_kind(g: &Inner, info: &DevInfo) -> Option<FirmwareKind> {
    if g.connected {
        Some(if info.rustudio {
            FirmwareKind::Audio
        } else if info.golden() {
            FirmwareKind::Golden
        } else {
            FirmwareKind::Official
        })
    } else {
        match g.usb.as_deref() {
            Some("mini-lab") => Some(FirmwareKind::Lab),
            Some("mini-audio") => Some(FirmwareKind::Audio),
            _ => None,
        }
    }
}

impl Ctl {
    pub(crate) fn module_devices_locked(&self, g: &Inner, info: &DevInfo) -> [ModuleRuntime; 3] {
        [goxlr_runtime(self.health_status(g), g, self.simulated(), info), self.mouse.runtime(), self.keyboard.runtime()]
    }

    pub fn modules_snapshot(&self) -> Value {
        let info = self.dev_info.lock().unwrap().clone();
        let g = self.lock();
        json!({"installed": modules::installed(), "devices": self.module_devices_locked(&g, &info)})
    }

    pub fn module_state_snapshot(&self, id: &str) -> ApiResult {
        let module = modules::find(id).ok_or_else(|| unknown_module(id))?;
        match module.id {
            "goxlr-mini" => {
                let info = self.dev_info.lock().unwrap().clone();
                let v = self.version();
                let g = self.lock();
                let live = &g.live;
                Ok(json!({"module": module, "device": self.module_devices_locked(&g, &info)[0],
                    "state": g.state.to_value(), "meta": crate::views::meta(&g.state, g.connected && g.mix2),
                    "live": {"fader_positions": live.fader_positions, "mic_db": live.mic_db,
                        "pressed": live.pressed, "encoders": live.encoders}, "v": v}))
            }
            "attack-shark-r5-ultra" => {
                let data = self.mouse.state();
                Ok(json!({"module":module,"device":self.mouse.runtime(),"state":data["state"],"v":data["v"],"meta":{},"live":{}}))
            }
            crate::keyboard::ID => {
                let data = self.keyboard.state();
                Ok(json!({"module":module,"device":self.keyboard.runtime(),"state":data["state"],"v":data["v"],"meta":{},"live":{}}))
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
            crate::keyboard::ID => self.keyboard.command(action, p).map_err(|msg| ApiError { status: 400, msg }),
            _ => Err(unknown_module(id)),
        }
    }
}

fn unknown_module(id: &str) -> ApiError {
    ApiError { status: 404, msg: format!("module inconnu : {id}") }
}
