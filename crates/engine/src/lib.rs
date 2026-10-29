//! Rustudio : moteur. Un seul fil parle a l'USB ; l'API locale ne touche jamais l'appareil.
pub mod apply;
pub mod config;
pub mod controller;
pub mod drops;
pub mod eqapo;
pub mod glitches;
pub mod http;
pub mod jobs;
pub mod library;
pub mod log;
pub mod net;
#[cfg(windows)]
pub mod service;
pub mod supervisor;
#[cfg(windows)]
pub mod uninstall;
pub mod updater;
pub mod usbmap;
pub mod views;
pub mod watch;
pub mod win;

use std::sync::Arc;

/// Chargeur du pilote : la fausse GoXLR en mode simule, la DLL officielle sinon.
pub fn make_loader(cfg: &config::Config) -> (rustudio_goxlr::Loader, Option<rustudio_goxlr::sim::Sim>) {
    if cfg.simulate {
        let fw = std::env::var("RUSTUDIO_SIM_FW").ok().and_then(|s| {
            let p: Vec<u32> = s.split('.').filter_map(|x| x.parse().ok()).collect();
            (p.len() == 4).then(|| [p[0], p[1], p[2], p[3]])
        });
        let sim = rustudio_goxlr::sim::Sim::new(fw.unwrap_or([1, 2, 1, 49]));
        let s2 = sim.clone();
        return (Box::new(move || Ok(Box::new(s2.tusb()) as Box<dyn rustudio_goxlr::Tusb>)), Some(sim));
    }
    #[cfg(windows)]
    {
        (Box::new(|| rustudio_goxlr::dll::Dll::load(&rustudio_goxlr::dll::default_path()).map(|d| Box::new(d) as Box<dyn rustudio_goxlr::Tusb>)), None)
    }
    #[cfg(not(windows))]
    {
        (Box::new(|| Err("pilote GoXLR disponible sous Windows seulement (lance avec --simulate)".to_string())), None)
    }
}

/// Moteur complet : le port HTTP d'abord, l'USB ensuite (un second moteur s'arrete sans toucher au pilote).
pub fn run_engine(cfg: config::Config) -> i32 {
    log::set_file(cfg.log_path());
    let server = match tiny_http::Server::http(("127.0.0.1", cfg.port)) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            logf!("Port {} déjà pris ({e}) : un autre moteur tourne, arrêt sans toucher à l'USB", cfg.port);
            return config::EXIT_PORT_BUSY;
        }
    };
    let (loader, sim) = make_loader(&cfg);
    let ctl = controller::Ctl::new(cfg.clone(), loader, sim, true);
    logf!(
        "Démarrage du moteur Rustudio {}{}{}, état : {}",
        config::VERSION,
        if cfg.simulate { " (appareil simulé)" } else { "" },
        if cfg.engine_restarts > 0 { format!(", relance n°{}", cfg.engine_restarts) } else { String::new() },
        cfg.state_path().display()
    );
    let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
        std::thread::Builder::new().name(name.into()).spawn(f).expect("fil");
    };
    let c = ctl.clone();
    spawn("usb", Box::new(move || c.device_loop(None)));
    let c = ctl.clone();
    spawn("save", Box::new(move || c.save_loop()));
    let c = ctl.clone();
    spawn("apps", Box::new(move || c.app_watch_loop(std::time::Duration::from_secs(3))));
    let c = ctl.clone();
    spawn("updates", Box::new(move || c.updater.run_loop()));
    if cfg.supervised {
        spawn("parent", Box::new(supervisor::watch_parent));
    }
    if !cfg.simulate {
        // journal des coupures du son : allume des le depart, pour que le diagnostic ait un historique
        spawn("glitches", Box::new(|| {
            glitches::ensure_enabled();
        }));
        // programmes lances au moment des coupures comptees par le pilote
        let c = ctl.clone();
        spawn("drops", Box::new(move || c.drops_watch_loop()));
    }
    logf!("Interface et API sur http://127.0.0.1:{}/", cfg.port);
    http::serve(ctl.clone(), server, 8);
    ctl.save_if_dirty();
    0
}
