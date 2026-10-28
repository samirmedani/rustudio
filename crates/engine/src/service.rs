//! Service Windows « Rustudio » (compte systeme, demarrage automatique, relance en cas de panne).
use crate::config::{Config, SERVICE_NAME};
use std::ffi::{OsStr, OsString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows_service::service::*;
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

pub fn run_dispatcher() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|e| e.to_string())
}

fn service_main(_args: Vec<OsString>) {
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let handler = move |ev| match ev {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            s2.store(true, Ordering::SeqCst);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(h) = service_control_handler::register(SERVICE_NAME, handler) else { return };
    let status = |state: ServiceState, accept: ServiceControlAccept| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    };
    let _ = h.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN));
    crate::supervisor::supervise(Config::from_env(), stop);
    let _ = h.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()));
}

pub fn install(exe: &std::path::Path) -> Result<(), String> {
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .map_err(|e| format!("gestionnaire de services : {e}"))?;
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from("Rustudio"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.to_path_buf(),
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let access = ServiceAccess::QUERY_STATUS | ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::STOP;
    let svc = match mgr.open_service(SERVICE_NAME, access) {
        Ok(s) => {
            s.change_config(&info).map_err(|e| format!("configuration du service : {e}"))?;
            s
        }
        Err(_) => mgr.create_service(&info, access).map_err(|e| format!("création du service : {e}"))?,
    };
    let _ = svc.set_description("Rustudio : pilote la GoXLR Mini et sert l'interface sur http://127.0.0.1:14565");
    let _ = svc.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(2) },
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(5) },
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(30) },
        ]),
    });
    let _ = svc.set_failure_actions_on_non_crash_failures(true);
    if svc.query_status().map(|s| s.current_state != ServiceState::Running).unwrap_or(true) {
        svc.start(&[] as &[&OsStr]).map_err(|e| format!("démarrage du service : {e}"))?;
    }
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(|e| e.to_string())?;
    let svc = match mgr.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let _ = svc.stop();
    for _ in 0..40 {
        if svc.query_status().map(|s| s.current_state == ServiceState::Stopped).unwrap_or(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    svc.delete().map_err(|e| e.to_string())
}
