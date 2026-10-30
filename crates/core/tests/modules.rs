use rustudio_core::modules::{self, ModuleStatus};
use std::collections::HashSet;

#[test]
fn registre_local_coherent_et_sans_modules_a_contribuer() {
    let mut ids = HashSet::new();
    for module in modules::installed() {
        assert!(ids.insert(module.id), "identifiant de module dupliqué");
        assert!(!module.id.is_empty() && module.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(!module.name.is_empty() && !module.vendor.is_empty() && !module.summary.is_empty());
        assert_eq!(modules::find(module.id).unwrap().name, module.name);
        let capabilities: HashSet<_> = module.capabilities.iter().copied().collect();
        assert_eq!(capabilities.len(), module.capabilities.len(), "capacité dupliquée");
        let actions: HashSet<_> = module.actions.iter().copied().collect();
        assert_eq!(actions.len(), module.actions.len(), "action dupliquée");
        assert!(!actions.is_empty());
        for global in ["update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh", "restart-engine"] {
            assert!(!actions.contains(global), "une action globale ne dépend pas d'un module");
        }
        let mut pages = HashSet::new();
        for page in module.pages {
            assert!(pages.insert(page.id), "page dupliquée");
            assert!(!page.label.is_empty() && !page.icon.is_empty());
        }
    }
    let goxlr = modules::find("goxlr-mini").unwrap();
    assert_eq!(goxlr.kind, "audio-interface");
    assert_eq!(goxlr.pages.iter().map(|p| p.id).collect::<Vec<_>>(), ["mixer", "mic", "hp", "routing", "light", "scenes"]);
    assert_eq!(goxlr.capabilities, ["mixer", "microphone", "headphone-eq", "routing", "lighting", "scenes"]);
    for action in ["volume", "mute", "mic", "route", "hp-eq", "scene", "profile", "raw", "job-cancel", "echo-check", "sound-test"] {
        assert!(goxlr.actions.contains(&action));
    }
    for requested in ["goxlr-full", "attack-shark-r5-ultra", "ddc-monitor", "inconnu"] {
        assert!(modules::find(requested).is_none(), "une fiche du catalogue n'est pas un module livré");
    }
}

#[test]
fn statuts_serialises_selon_le_contrat_accueil() {
    for (status, expected) in [
        (ModuleStatus::Ready, "ready"), (ModuleStatus::Disconnected, "disconnected"),
        (ModuleStatus::Starting, "starting"), (ModuleStatus::Error, "error"), (ModuleStatus::Blocked, "blocked"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
    }
}
