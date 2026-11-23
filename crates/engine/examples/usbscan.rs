//! Ce que Windows voit sur l'USB, lu dans la liste des périphériques sans ouvrir le pilote GoXLR : « mini »,
//! « mini-lab », « mini-audio » (firmwares Rustudio), « full », « failed » ou « none ». Sans risque pour un moteur
//! en marche : aucune énumération du pilote.
//!
//!     cargo run -p rustudio-engine --example usbscan

fn main() {
    println!("{}", rustudio_engine::win::usb_scan().unwrap_or_else(|| "liste USB illisible".into()));
}

