//! Hôte COM sans périphérique. Charge la DLL livrée, sans toucher au registre ni à WASAPI.
#[cfg(windows)]
mod host {
    use std::{
        ffi::c_void,
        mem::{transmute, ManuallyDrop},
        ptr,
    };
    use windows::core::{
        implement, s, IUnknown, Interface, Ref, Result, BOOL, GUID, HRESULT, HSTRING,
    };
    use windows::Win32::{
        Foundation::*,
        Media::Audio::{Apo::*, WAVEFORMATEX},
        System::{Com::*, LibraryLoader::*},
    };
    const CLASS: GUID = GUID::from_u128(0xf032fd61_4408_4491_ad71_e802d82b33a3);
    #[implement(IAudioMediaType)]
    struct Media(UNCOMPRESSEDAUDIOFORMAT);
    impl IAudioMediaType_Impl for Media_Impl {
        fn IsCompressedFormat(&self) -> Result<BOOL> {
            Ok(false.into())
        }
        fn IsEqual(&self, _: Ref<IAudioMediaType>) -> Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn GetAudioFormat(&self) -> *mut WAVEFORMATEX {
            ptr::null_mut()
        }
        fn GetUncompressedAudioFormat(&self, out: *mut UNCOMPRESSEDAUDIOFORMAT) -> Result<()> {
            if out.is_null() {
                return Err(E_POINTER.into());
            }
            unsafe {
                *out = self.0;
            }
            Ok(())
        }
    }
    fn media(channels: u32) -> IAudioMediaType {
        Media(UNCOMPRESSEDAUDIOFORMAT {
            guidFormatType: GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71),
            dwSamplesPerFrame: channels,
            dwBytesPerSampleContainer: 4,
            dwValidBitsPerSample: 32,
            fFramesPerSecond: 48000.0,
            dwChannelMask: if channels == 1 { 4 } else { 3 },
        })
        .into()
    }
    unsafe fn exercise(factory: &IClassFactory, channels: u32) -> Result<()> {
        let effect: IAudioProcessingObject = factory.CreateInstance(None)?;
        effect.Initialize(&[])?;
        assert!(effect.Initialize(&[]).is_err());
        let _: IAudioSystemEffects = effect.cast()?;
        let _: IAudioSystemEffects2 = effect.cast()?;
        let config: IAudioProcessingObjectConfiguration = effect.cast()?;
        let rt: IAudioProcessingObjectRT = effect.cast()?;
        let f = media(channels);
        effect.IsInputFormatSupported(None, &f)?;
        effect.IsOutputFormatSupported(&f, &f)?;
        assert!(effect.IsInputFormatSupported(None, &media(3)).is_err());
        let mut desc = APO_CONNECTION_DESCRIPTOR {
            pFormat: ManuallyDrop::new(Some(f)),
            u32MaxFrameCount: 100,
            ..Default::default()
        };
        config.LockForProcess(&[&desc], &[&desc])?;
        assert_eq!(effect.GetInputChannelCount()?, channels);
        let mut input = [0.0f32; 200];
        for (i, s) in input.iter_mut().enumerate() {
            *s = i as f32 / 201.0 - 0.5;
        }
        let mut output = [123.0f32; 200];
        let mut src = APO_CONNECTION_PROPERTY {
            pBuffer: input.as_ptr() as usize,
            u32ValidFrameCount: 89,
            u32BufferFlags: BUFFER_VALID,
            ..Default::default()
        };
        let mut dst = APO_CONNECTION_PROPERTY {
            pBuffer: output.as_mut_ptr() as usize,
            ..Default::default()
        };
        rt.APOProcess(1, &ptr::from_ref(&src), 1, &mut ptr::from_mut(&mut dst));
        let len = 89 * channels as usize;
        assert_eq!(dst.u32ValidFrameCount, 89);
        assert_eq!(dst.u32BufferFlags, BUFFER_VALID);
        assert_eq!(output[..len], input[..len]);
        assert_eq!(output[len], 123.0);
        let same = ptr::from_mut(&mut src);
        rt.APOProcess(1, &same.cast_const(), 1, &mut same.clone());
        assert_eq!(src.u32ValidFrameCount, 89);
        src.u32BufferFlags = BUFFER_SILENT;
        src.pBuffer = 0;
        rt.APOProcess(1, &ptr::from_ref(&src), 1, &mut ptr::from_mut(&mut dst));
        assert_eq!(dst.u32BufferFlags, BUFFER_SILENT);
        src.u32ValidFrameCount = 101;
        rt.APOProcess(1, &ptr::from_ref(&src), 1, &mut ptr::from_mut(&mut dst));
        assert_eq!(dst.u32BufferFlags, BUFFER_INVALID);
        assert_eq!(dst.u32ValidFrameCount, 0);
        config.UnlockForProcess()?;
        assert!(config.UnlockForProcess().is_err());
        ManuallyDrop::drop(&mut desc.pFormat);
        Ok(())
    }
    unsafe fn aggregated(factory: &IClassFactory) -> Result<()> {
        let outer: IUnknown = media(1).cast()?;
        let references = || {
            (outer.vtable().AddRef)(outer.as_raw());
            (outer.vtable().Release)(outer.as_raw())
        };
        let original = references();
        assert!(factory
            .CreateInstance::<_, IAudioProcessingObject>(&outer)
            .is_err());
        let inner: IUnknown = factory.CreateInstance(&outer)?;
        assert_ne!(inner.as_raw(), outer.as_raw());
        let effect: IAudioProcessingObject = inner.cast()?;
        assert_eq!(effect.cast::<IUnknown>()?.as_raw(), outer.as_raw());
        // Le contrôleur de test publie uniquement IAudioMediaType. Les requêtes
        // reçues sur une interface publique doivent lui être déléguées.
        assert!(effect.cast::<IAudioMediaType>().is_ok());
        assert!(effect.cast::<IAudioProcessingObjectRT>().is_err());
        let config: IAudioProcessingObjectConfiguration = inner.cast()?;
        let rt: IAudioProcessingObjectRT = inner.cast()?;
        let effects: IAudioSystemEffects2 = inner.cast()?;
        assert_eq!(config.cast::<IUnknown>()?.as_raw(), outer.as_raw());
        assert_eq!(rt.cast::<IUnknown>()?.as_raw(), outer.as_raw());
        assert_eq!(effects.cast::<IUnknown>()?.as_raw(), outer.as_raw());
        let props = effect.GetRegistrationProperties()?;
        assert!(!props.is_null());
        assert_eq!((*props).clsid, CLASS);
        CoTaskMemFree(Some(props.cast()));
        let (mut ids, mut count) = (ptr::null_mut(), 0);
        effects.GetEffectsList(&mut ids, &mut count, HANDLE::default())?;
        assert_eq!(count, 1);
        assert_eq!(*ids, CLASS);
        CoTaskMemFree(Some(ids.cast()));
        effect.Initialize(&[])?;
        let mut desc = APO_CONNECTION_DESCRIPTOR {
            pFormat: ManuallyDrop::new(Some(media(1))),
            u32MaxFrameCount: 1,
            ..Default::default()
        };
        config.LockForProcess(&[&desc], &[&desc])?;
        let x = 0.25f32;
        let mut y = 0f32;
        let src = APO_CONNECTION_PROPERTY {
            pBuffer: ptr::from_ref(&x) as usize,
            u32BufferFlags: BUFFER_VALID,
            u32ValidFrameCount: 1,
            ..Default::default()
        };
        let mut dst = APO_CONNECTION_PROPERTY {
            pBuffer: ptr::from_mut(&mut y) as usize,
            ..Default::default()
        };
        rt.APOProcess(1, &ptr::from_ref(&src), 1, &mut ptr::from_mut(&mut dst));
        assert_eq!((y, dst.u32ValidFrameCount), (x, 1));
        config.UnlockForProcess()?;
        ManuallyDrop::drop(&mut desc.pFormat);
        drop((effects, rt, config, effect, inner));
        assert_eq!(references(), original);
        Ok(())
    }
    // Contrôle du chemin complet réglages → DLL → niveaux, avec un GUID de test sans endpoint.
    unsafe fn controlled(factory: &IClassFactory) -> Result<()> {
        use rustudio_windows_mic::{control::Controls, mapping::Mapping, Voice};
        use std::sync::atomic::Ordering;
        use windows::Win32::{
            Media::Audio::PKEY_AudioEndpoint_GUID, UI::Shell::PropertiesSystem::*,
        };
        let guid = format!("{:?}", CoCreateGuid()?).to_ascii_lowercase();
        let guid = format!("{{{}}}", guid.trim_matches(['{', '}']));
        let shared = Mapping::open(&guid, true)?;
        let voice = Voice {
            enabled: true,
            hpf: false,
            gain_db: 6,
            ..Voice::default()
        };
        shared
            .shared()
            .publish(Controls::new(&voice, false, 90, 200));
        let mut raw = ptr::null_mut();
        PSCreateMemoryPropertyStore(&IPropertyStore::IID, &mut raw)?;
        let store = IPropertyStore::from_raw(raw);
        store.SetValue(&PKEY_AudioEndpoint_GUID, &guid.as_str().into())?;
        let mut init = APOInitSystemEffects::default();
        init.APOInit.cbSize = size_of_val(&init) as u32;
        init.APOInit.clsid = CLASS;
        init.pAPOEndpointProperties = ManuallyDrop::new(Some(store));
        let effect: IAudioProcessingObject = factory.CreateInstance(None)?;
        let r = effect.Initialize(std::slice::from_raw_parts(
            ptr::from_ref(&init).cast(),
            size_of_val(&init),
        ));
        ManuallyDrop::drop(&mut init.pAPOEndpointProperties);
        r?;
        let config: IAudioProcessingObjectConfiguration = effect.cast()?;
        let rt: IAudioProcessingObjectRT = effect.cast()?;
        let mut desc = APO_CONNECTION_DESCRIPTOR {
            pFormat: ManuallyDrop::new(Some(media(1))),
            u32MaxFrameCount: 480,
            ..Default::default()
        };
        config.LockForProcess(&[&desc], &[&desc])?;
        assert_eq!(effect.GetLatency()?, 15000);
        let mut samples = [0.125f32; 480];
        let mut prop = APO_CONNECTION_PROPERTY {
            pBuffer: samples.as_mut_ptr() as usize,
            u32ValidFrameCount: 480,
            u32BufferFlags: BUFFER_VALID,
            ..Default::default()
        };
        let same = ptr::from_mut(&mut prop);
        rt.APOProcess(1, &same.cast_const(), 1, &mut same.clone());
        assert_eq!(samples[..72], [0.; 72]);
        assert!((samples[479] - 0.125 * 10f32.powf(6. / 20.)).abs() < 1e-5);
        let mut next = voice;
        next.gain_db = 0;
        shared
            .shared()
            .publish(Controls::new(&next, false, 90, 200));
        samples.fill(0.125);
        rt.APOProcess(1, &same.cast_const(), 1, &mut same.clone());
        assert!((samples[479] - 0.125).abs() < 1e-6);
        assert!(samples.windows(2).all(|w| (w[0] - w[1]).abs() < 0.002));
        assert_eq!(shared.shared().callbacks.load(Ordering::Acquire), 2);
        assert_eq!(shared.shared().faults.load(Ordering::Acquire), 0);
        config.UnlockForProcess()?;
        ManuallyDrop::drop(&mut desc.pFormat);
        println!("Routage mémoire SYSTEM, gain, délai, réglage en vol et télémétrie vérifiés.");
        Ok(())
    }
    pub fn run() -> Result<()> {
        let path = std::env::args_os().nth(1).expect("chemin de la DLL");
        let path = std::fs::canonicalize(path)?;
        unsafe {
            let lib = LoadLibraryExW(
                &HSTRING::from(path.as_os_str()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )?;
            type GetClass =
                unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
            let get: GetClass =
                transmute(GetProcAddress(lib, s!("DllGetClassObject")).ok_or(E_NOINTERFACE)?);
            let unload: unsafe extern "system" fn() -> HRESULT =
                transmute(GetProcAddress(lib, s!("DllCanUnloadNow")).ok_or(E_NOINTERFACE)?);
            let mut raw = ptr::null_mut();
            get(&CLASS, &IClassFactory::IID, &mut raw).ok()?;
            let factory = IClassFactory::from_raw(raw);
            aggregated(&factory)?;
            for i in 0..200 {
                exercise(&factory, 1 + i % 2)?;
            }
            if std::env::args().nth(2).as_deref() == Some("--controlled") {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
                let result = controlled(&factory);
                CoUninitialize();
                result?;
            }
            assert_eq!(unload(), S_FALSE);
            drop(factory);
            assert_eq!(unload(), S_OK);
            FreeLibrary(lib)?;
        }
        println!("200 cycles COM mono/stéréo, buffers en place, séparés, silencieux et invalides.");
        Ok(())
    }
}
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    host::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Hôte de test réservé à Windows.");
}
