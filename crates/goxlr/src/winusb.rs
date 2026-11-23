//! Firmware Rustudio de la GoXLR Mini : les memes trames de commande que le pilote officiel, portees par des
//! requetes WinUSB sur l'interface de diagnostic du firmware (voir USB-LAB.md, prive).
use crate::Tusb;
use rustudio_core::consts::STATUS_STALL;
use std::collections::HashMap;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW, CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
    CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows_sys::Win32::Devices::Usb::{
    WinUsb_ControlTransfer, WinUsb_Free, WinUsb_Initialize, WinUsb_SetPipePolicy, PIPE_TRANSFER_TIMEOUT,
    WINUSB_INTERFACE_HANDLE, WINUSB_SETUP_PACKET,
};
use windows_sys::Win32::Foundation::{ERROR_GEN_FAILURE, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// DeviceInterfaceGUIDs des descripteurs Microsoft OS 2.0 du firmware Rustudio.
const INTERFACE_GUID: GUID = GUID::from_u128(0x36bbebd8_15b3_4316_a6ed_b1265f3cebff);
/// Identifiant USB du firmware Rustudio dans les chemins d'interface.
pub const RUSTUDIO_ID: &str = "VID_1209&PID_0002";

// Requetes fournisseur du firmware (destinataire : l'appareil).
const COMMAND: u8 = 0x10;
const ANSWER: u8 = 0x11;
const HELLO: u8 = 0x12;
const OPEN: u8 = 0x13;
const TRANSFER_TIMEOUT_MS: u32 = 2_000;

/// Chemins des interfaces WinUSB du firmware Rustudio presentes.
pub fn interface_paths() -> Vec<String> {
    loop {
        let mut length = 0;
        // SAFETY : parametre de sortie simple, GUID statique, aucun filtre d'instance.
        let mut status = unsafe {
            CM_Get_Device_Interface_List_SizeW(&mut length, &INTERFACE_GUID, null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
        };
        let mut list = vec![0u16; length as usize];
        if status == CR_SUCCESS {
            // SAFETY : list contient les length caracteres annonces.
            status = unsafe {
                CM_Get_Device_Interface_ListW(
                    &INTERFACE_GUID, null(), list.as_mut_ptr(), length, CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
                )
            };
        }
        match status {
            CR_SUCCESS => {
                return list
                    .split(|&unit| unit == 0)
                    .filter(|path| !path.is_empty())
                    .map(String::from_utf16_lossy)
                    .filter(|path| path.to_uppercase().contains(RUSTUDIO_ID))
                    .collect()
            }
            // une interface est apparue entre les deux appels : nouvelle taille
            CR_BUFFER_SMALL => continue,
            _ => return Vec::new(),
        }
    }
}

struct Device {
    path: String,
    winusb: WINUSB_INTERFACE_HANDLE,
    _file: OwnedHandle,
}

// SAFETY : les handles WinUSB et de fichier ne sont lies a aucun fil ; le moteur n'en utilise qu'un a la fois.
unsafe impl Send for Device {}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY : handle rendu par WinUsb_Initialize, libere une seule fois.
        unsafe { WinUsb_Free(self.winusb) };
    }
}

/// Appareils du firmware Rustudio, vus comme ceux du pilote officiel par GoXlr.
#[derive(Default)]
pub struct Lab {
    paths: Vec<String>,
    open: HashMap<u32, Device>,
    next: u32,
}

impl Lab {
    fn transfer(&mut self, h: u32, request_type: u8, request: u8, data: &mut [u8]) -> Result<usize, u32> {
        let device = self.open.get(&h).ok_or(ERROR_GEN_FAILURE)?;
        let setup = WINUSB_SETUP_PACKET { RequestType: request_type, Request: request, Value: 0, Index: 0, Length: data.len() as u16 };
        let mut transferred = 0;
        // SAFETY : transfert synchrone (sans OVERLAPPED) dans un tampon qui lui survit.
        let done = unsafe {
            WinUsb_ControlTransfer(device.winusb, setup, data.as_mut_ptr(), data.len() as u32, &mut transferred, null())
        };
        if done == 0 {
            let code = io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
            // un STALL dit « pas encore de reponse », comme le pilote officiel
            return Err(if code == ERROR_GEN_FAILURE { STATUS_STALL } else { code });
        }
        Ok(transferred as usize)
    }
}

impl Tusb for Lab {
    fn enumerate(&mut self) -> u32 {
        self.paths = interface_paths();
        0
    }
    fn device_count(&mut self) -> u32 {
        self.paths.len() as u32
    }
    fn open_by_index(&mut self, idx: u32) -> Result<u32, u32> {
        let path = self.paths.get(idx as usize).cloned().ok_or(ERROR_GEN_FAILURE)?;
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        // SAFETY : chemin termine par un nul, sans attributs de securite ni modele.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED, null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32);
        }
        // SAFETY : handle valide rendu par CreateFileW, possede ici seulement.
        let file = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut winusb = null_mut();
        // SAFETY : handle ouvert avec FILE_FLAG_OVERLAPPED, parametre de sortie simple.
        if unsafe { WinUsb_Initialize(file.as_raw_handle(), &mut winusb) } == 0 {
            return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32);
        }
        let device = Device { path, winusb, _file: file };
        // SAFETY : le tube 0 est EP0 ; WinUSB lit la valeur ULONG pendant l'appel.
        unsafe {
            WinUsb_SetPipePolicy(
                device.winusb, 0, PIPE_TRANSFER_TIMEOUT, size_of::<u32>() as u32, (&TRANSFER_TIMEOUT_MS as *const u32).cast(),
            )
        };
        self.next = self.next.wrapping_add(1).max(1);
        self.open.insert(self.next, device);
        Ok(self.next)
    }
    fn instance_id(&mut self, h: u32) -> Option<String> {
        self.open.get(&h).map(|device| device.path.to_uppercase())
    }
    fn vendor_out(&mut self, h: u32, request: u32, data: &[u8]) -> u32 {
        let request = match request {
            1 => OPEN,
            2 => COMMAND,
            _ => return ERROR_GEN_FAILURE,
        };
        let mut copy = data.to_vec();
        self.transfer(h, 0x40, request, &mut copy).err().unwrap_or(0)
    }
    fn vendor_in(&mut self, h: u32, request: u32, len: usize) -> Result<Vec<u8>, u32> {
        let request = match request {
            0 => HELLO,
            3 => ANSWER,
            _ => return Err(ERROR_GEN_FAILURE),
        };
        let mut buf = vec![0u8; len];
        let n = self.transfer(h, 0xC0, request, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }
    fn close(&mut self, h: u32) {
        self.open.remove(&h);
    }
    fn status_text(&self, code: u32) -> String {
        if code == STATUS_STALL {
            return "STALL".into();
        }
        io::Error::from_raw_os_error(code as i32).to_string()
    }
}
