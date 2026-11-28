//! Section paginée créée par le moteur SYSTEM. Le propriétaire est vérifié à chaque ouverture.
//! Aucun accès au système depuis le rappel audio, seulement des atomiques dans la vue déjà ouverte.
use crate::control::Shared;
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE},
    Security::{
        Authorization::*, IsWellKnownSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION,
        SECURITY_ATTRIBUTES,
    },
    System::Memory::*,
};

pub struct Mapping {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    handle: HANDLE,
}
// La vue a la durée de vie du handle et toutes ses données mutables sont atomiques.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn open(guid: &str, create: bool) -> io::Result<Self> {
        if !rustudio_core::micns::is_guid(guid) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let name = wide(&format!(
            "Global\\Rustudio.Micro.1.{}",
            guid.to_ascii_lowercase()
        ));
        unsafe {
            let handle = if create {
                let mut sd = ptr::null_mut();
                let dacl = wide("O:SYD:P(A;;GA;;;SY)(A;;0x20006;;;LS)");
                if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    dacl.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut(),
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
                let sa = SECURITY_ATTRIBUTES {
                    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd,
                    bInheritHandle: 0,
                };
                let h = CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    &sa,
                    PAGE_READWRITE,
                    0,
                    size_of::<Shared>() as u32,
                    name.as_ptr(),
                );
                let err = io::Error::last_os_error();
                LocalFree(sd);
                if h.is_null() {
                    return Err(err);
                }
                h
            } else {
                let h =
                    OpenFileMappingW(0x20000 | FILE_MAP_READ | FILE_MAP_WRITE, 0, name.as_ptr());
                if h.is_null() {
                    return Err(io::Error::last_os_error());
                }
                h
            };
            let mut owner = ptr::null_mut();
            let mut sd = ptr::null_mut();
            let code = GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut sd,
            );
            let ok = code == 0 && !owner.is_null() && IsWellKnownSid(owner, WinLocalSystemSid) != 0;
            if !sd.is_null() {
                LocalFree(sd);
            }
            if !ok {
                CloseHandle(handle);
                return Err(if code == 0 {
                    io::ErrorKind::PermissionDenied.into()
                } else {
                    io::Error::from_raw_os_error(code as i32)
                });
            }
            let view = MapViewOfFile(
                handle,
                FILE_MAP_READ | FILE_MAP_WRITE,
                0,
                0,
                size_of::<Shared>(),
            );
            if view.Value.is_null() {
                let e = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(e);
            }
            Ok(Self { view, handle })
        }
    }

    pub fn shared(&self) -> &Shared {
        unsafe { &*self.view.Value.cast::<Shared>() }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}
