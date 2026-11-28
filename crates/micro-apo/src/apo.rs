use crate::stream::Stream;
use atomic_refcell::AtomicRefCell;
use rustudio_windows_mic::mapping::Mapping;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;
use windows::core::{implement, IUnknown, Interface, Ref, Result, BOOL, GUID, HRESULT};
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::Media::Audio::PKEY_AudioEndpoint_GUID;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

const CLASS: GUID = GUID::from_u128(0xf032fd61_4408_4491_ad71_e802d82b33a3);
const FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicUsize = AtomicUsize::new(0);

struct Lifetime;
impl Lifetime {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Release);
    }
}

#[implement(IClassFactory)]
struct Factory(Lifetime);

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if out.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *out = ptr::null_mut();
        }
        if let Some(outer) = outer.as_ref() {
            if unsafe { *iid } != IUnknown::IID {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            unsafe { *out = crate::aggregate::create(outer, Effect::new().into())? };
            return Ok(());
        }
        let object: IAudioProcessingObject = Effect::new().into();
        unsafe { object.query(iid, out).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::Relaxed);
        } else {
            let mut count = LOCKS.load(Ordering::Acquire);
            loop {
                let next = count.checked_sub(1).ok_or(E_UNEXPECTED)?;
                match LOCKS.compare_exchange_weak(count, next, Ordering::AcqRel, Ordering::Acquire)
                {
                    Ok(_) => break,
                    Err(actual) => count = actual,
                }
            }
        }
        Ok(())
    }
}

#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    id: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() || id.is_null() || iid.is_null() {
        return E_POINTER;
    }
    *out = ptr::null_mut();
    if *id != CLASS {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory(Lifetime::new()).into();
    factory.query(iid, out)
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::Acquire) == 0 && LOCKS.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

fn format(media: &IAudioMediaType) -> Result<UNCOMPRESSEDAUDIOFORMAT> {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    unsafe {
        media.GetUncompressedAudioFormat(&mut f)?;
    }
    if f.guidFormatType != FLOAT
        || !(1..=2).contains(&f.dwSamplesPerFrame)
        || f.dwBytesPerSampleContainer != 4
        || f.dwValidBitsPerSample != 32
        || !crate::stream::RATES.contains(&(f.fFramesPerSecond as u32))
        || f.fFramesPerSecond.fract() != 0.0
    {
        return Err(APOERR_FORMAT_NOT_SUPPORTED.into());
    }
    Ok(f)
}

fn supported(
    opposite: Ref<IAudioMediaType>,
    requested: Ref<IAudioMediaType>,
) -> Result<IAudioMediaType> {
    let m = requested.ok()?;
    let f = format(m)?;
    if let Some(other) = opposite.as_ref() {
        if format(other)? != f {
            return Err(APOERR_FORMAT_NOT_SUPPORTED.into());
        }
    }
    Ok(m.clone())
}

fn registration() -> APO_REG_PROPERTIES {
    let mut p = APO_REG_PROPERTIES {
        clsid: CLASS,
        Flags: APO_FLAG(APO_FLAG_DEFAULT.0 | APO_FLAG_INPLACE.0),
        u32MajorVersion: 1,
        u32MinInputConnections: 1,
        u32MaxInputConnections: 1,
        u32MinOutputConnections: 1,
        u32MaxOutputConnections: 1,
        u32MaxInstances: u32::MAX,
        u32NumAPOInterfaces: 1,
        iidAPOInterfaceList: [IAudioProcessingObject::IID],
        ..Default::default()
    };
    for (dst, c) in p
        .szFriendlyName
        .iter_mut()
        .zip("Rustudio Micro".encode_utf16())
    {
        *dst = c;
    }
    for (dst, c) in p
        .szCopyrightInfo
        .iter_mut()
        .zip("Rustudio (MIT)".encode_utf16())
    {
        *dst = c;
    }
    p
}

fn task_alloc<T>(value: T) -> Result<*mut T> {
    unsafe {
        let p = CoTaskMemAlloc(size_of::<T>()).cast::<T>();
        if p.is_null() {
            return Err(E_OUTOFMEMORY.into());
        }
        p.write(value);
        Ok(p)
    }
}

struct Active {
    map: Mapping,
    stream: Stream,
    rate: u32,
    revision: Option<u32>,
    scratch: Vec<f32>,
    failed: bool,
}

fn protected<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

unsafe fn endpoint_guid(size: usize, data: *const u8) -> Option<String> {
    // Les trois structures ont des tailles et des dispositions différentes.
    let store: &IPropertyStore = if size == size_of::<APOInitSystemEffects>() {
        (*data.cast::<APOInitSystemEffects>())
            .pAPOEndpointProperties
            .as_ref()?
    } else if size == size_of::<APOInitSystemEffects2>() {
        (*data.cast::<APOInitSystemEffects2>())
            .pAPOEndpointProperties
            .as_ref()?
    } else if size == size_of::<APOInitSystemEffects3>() {
        (*data.cast::<APOInitSystemEffects3>())
            .pAPOEndpointProperties
            .as_ref()?
    } else {
        return None;
    };
    let value = store.GetValue(&PKEY_AudioEndpoint_GUID).ok()?;
    let p = PropVariantToStringAlloc(&value).ok()?;
    let guid = p.to_string().ok();
    CoTaskMemFree(Some(p.0.cast()));
    guid.filter(|g| rustudio_core::micns::is_guid(g))
}

#[implement(
    IAudioProcessingObject,
    IAudioProcessingObjectConfiguration,
    IAudioProcessingObjectRT,
    IAudioSystemEffects2
)]
struct Effect {
    _lifetime: Lifetime,
    initialized: AtomicBool,
    channels: AtomicU32,
    max_frames: AtomicU32,
    latency: AtomicU32,
    endpoint: OnceLock<String>,
    active: AtomicRefCell<Option<Active>>,
}

impl Effect {
    fn new() -> Self {
        Self {
            _lifetime: Lifetime::new(),
            initialized: false.into(),
            channels: 0.into(),
            max_frames: 0.into(),
            latency: 0.into(),
            endpoint: OnceLock::new(),
            active: AtomicRefCell::new(None),
        }
    }
}

impl IAudioProcessingObject_Impl for Effect_Impl {
    fn Reset(&self) -> Result<()> {
        protected(|| {
            let mut state = self
                .active
                .try_borrow_mut()
                .map_err(|_| APOERR_APO_LOCKED)?;
            if let Some(a) = state.as_mut() {
                let c = a
                    .map
                    .shared()
                    .changed(None)
                    .map(|(_, c)| c)
                    .unwrap_or_default();
                a.stream = Stream::new(a.rate, self.channels.load(Ordering::Acquire) as usize, c)
                    .ok_or(E_INVALIDARG)?;
                a.revision = None;
                a.failed = false;
                self.latency.store(
                    (u64::from(a.stream.latency()) * 10_000_000 / u64::from(a.rate)) as u32,
                    Ordering::Release,
                );
            }
            Ok(())
        })
    }
    fn GetLatency(&self) -> Result<i64> {
        Ok(self.latency.load(Ordering::Acquire).into())
    }
    fn GetRegistrationProperties(&self) -> Result<*mut APO_REG_PROPERTIES> {
        task_alloc(registration())
    }
    fn Initialize(&self, size: u32, data: *const u8) -> Result<()> {
        if size > 0 {
            if data.is_null() || (size as usize) < size_of::<APOInitBaseStruct>() {
                return Err(E_INVALIDARG.into());
            }
            let base = unsafe { data.cast::<APOInitBaseStruct>().read_unaligned() };
            if base.cbSize != size || base.clsid != CLASS {
                return Err(E_INVALIDARG.into());
            }
        }
        if !data.is_null() {
            if let Some(guid) = protected(|| Ok(unsafe { endpoint_guid(size as usize, data) }))? {
                let _ = self.endpoint.set(guid);
            }
        }
        self.initialized
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| APOERR_ALREADY_INITIALIZED)?;
        Ok(())
    }
    fn IsInputFormatSupported(
        &self,
        opposite: Ref<IAudioMediaType>,
        requested: Ref<IAudioMediaType>,
    ) -> Result<IAudioMediaType> {
        supported(opposite, requested)
    }
    fn IsOutputFormatSupported(
        &self,
        opposite: Ref<IAudioMediaType>,
        requested: Ref<IAudioMediaType>,
    ) -> Result<IAudioMediaType> {
        supported(opposite, requested)
    }
    fn GetInputChannelCount(&self) -> Result<u32> {
        let n = self.channels.load(Ordering::Acquire);
        if n == 0 {
            Err(APOERR_NOT_INITIALIZED.into())
        } else {
            Ok(n)
        }
    }
}

impl IAudioProcessingObjectConfiguration_Impl for Effect_Impl {
    fn LockForProcess(
        &self,
        ni: u32,
        input: *const *const APO_CONNECTION_DESCRIPTOR,
        no: u32,
        output: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> Result<()> {
        if !self.initialized.load(Ordering::Acquire) {
            return Err(APOERR_NOT_INITIALIZED.into());
        }
        if ni != 1 || no != 1 {
            return Err(APOERR_NUM_CONNECTIONS_INVALID.into());
        }
        unsafe {
            let i = input.as_ref().and_then(|p| p.as_ref()).ok_or(E_POINTER)?;
            let o = output.as_ref().and_then(|p| p.as_ref()).ok_or(E_POINTER)?;
            let f = format(i.pFormat.as_ref().ok_or(E_POINTER)?)?;
            if f != format(o.pFormat.as_ref().ok_or(E_POINTER)?)? {
                return Err(APOERR_INVALID_CONNECTION_FORMAT.into());
            }
            if i.u32MaxFrameCount == 0
                || i.u32MaxFrameCount > o.u32MaxFrameCount
                || i.u32MaxFrameCount > *crate::stream::RATES.end()
            {
                return Err(APOERR_INVALID_OUTPUT_MAXFRAMECOUNT.into());
            }
            let mut state = self
                .active
                .try_borrow_mut()
                .map_err(|_| APOERR_APO_LOCKED)?;
            if self.max_frames.load(Ordering::Acquire) != 0 {
                return Err(APOERR_APO_LOCKED.into());
            }
            let rate = f.fFramesPerSecond as u32;
            *state = protected(|| {
                let Some(map) = self
                    .endpoint
                    .get()
                    .and_then(|g| Mapping::open(g, false).ok())
                else {
                    return Ok(None);
                };
                let (revision, controls) = map.shared().changed(None).unzip();
                let stream = Stream::new(
                    rate,
                    f.dwSamplesPerFrame as usize,
                    controls.unwrap_or_default(),
                )
                .ok_or(E_INVALIDARG)?;
                let ticks = u64::from(stream.latency()) * 10_000_000 / u64::from(rate);
                self.latency.store(ticks as u32, Ordering::Release);
                Ok(Some(Active {
                    stream,
                    map,
                    rate,
                    revision,
                    scratch: vec![0.; (i.u32MaxFrameCount * f.dwSamplesPerFrame) as usize],
                    failed: false,
                }))
            })?;
            self.max_frames.store(i.u32MaxFrameCount, Ordering::Release);
            self.channels.store(f.dwSamplesPerFrame, Ordering::Release);
        }
        Ok(())
    }
    fn UnlockForProcess(&self) -> Result<()> {
        let mut state = self
            .active
            .try_borrow_mut()
            .map_err(|_| APOERR_APO_LOCKED)?;
        if self.max_frames.swap(0, Ordering::AcqRel) == 0 {
            return Err(APOERR_ALREADY_UNLOCKED.into());
        }
        *state = None;
        self.channels.store(0, Ordering::Release);
        self.latency.store(0, Ordering::Release);
        Ok(())
    }
}

impl IAudioSystemEffects_Impl for Effect_Impl {}
impl IAudioSystemEffects2_Impl for Effect_Impl {
    fn GetEffectsList(&self, ids: *mut *mut GUID, count: *mut u32, _: HANDLE) -> Result<()> {
        if ids.is_null() || count.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *ids = task_alloc(CLASS)?;
            *count = 1;
        }
        Ok(())
    }
}

impl IAudioProcessingObjectRT_Impl for Effect_Impl {
    fn CalcInputFrames(&self, n: u32) -> u32 {
        n
    }
    fn CalcOutputFrames(&self, n: u32) -> u32 {
        n
    }
    fn APOProcess(
        &self,
        ni: u32,
        input: *const *const APO_CONNECTION_PROPERTY,
        no: u32,
        output: *mut *mut APO_CONNECTION_PROPERTY,
    ) {
        if no != 1 {
            return;
        }
        unsafe {
            // Windows peut réutiliser le descripteur ET le buffer en place. Copier les métadonnées en premier.
            let i = if ni == 1 {
                input.as_ref().and_then(|p| p.as_ref()).copied()
            } else {
                None
            };
            let Some(o) = output.as_mut().and_then(|p| p.as_mut()) else {
                return;
            };
            o.u32BufferFlags = BUFFER_INVALID;
            o.u32ValidFrameCount = 0;
            let Some(i) = i else { return };
            let mut state = self.active.try_borrow_mut().ok();
            let max_frames = self.max_frames.load(Ordering::Acquire);
            if max_frames == 0 || i.u32ValidFrameCount > max_frames {
                return;
            }
            let n = i.u32ValidFrameCount as usize * self.channels.load(Ordering::Acquire) as usize;
            let silent = i.u32BufferFlags == BUFFER_SILENT;
            if !silent
                && (i.u32BufferFlags != BUFFER_VALID
                    || (n != 0 && (i.pBuffer == 0 || o.pBuffer == 0)))
            {
                return;
            }
            if let Some(a) = state.as_deref_mut().and_then(Option::as_mut).filter(|a| {
                !a.failed
                    && n != 0
                    && o.pBuffer != 0
                    && a.map.shared().magic.load(Ordering::Acquire)
                        == rustudio_windows_mic::control::MAGIC
            }) {
                let buf = &mut a.scratch[..n];
                if silent {
                    buf.fill(0.);
                } else {
                    ptr::copy_nonoverlapping(i.pBuffer as *const f32, buf.as_mut_ptr(), n);
                }
                let shared = a.map.shared();
                let processed = catch_unwind(AssertUnwindSafe(|| {
                    if let Some((rev, c)) = shared.changed(a.revision) {
                        a.stream.update(c);
                        a.revision = Some(rev);
                    }
                    a.stream.process(buf);
                }));
                if processed.is_ok() {
                    let empty = silent && buf.iter().all(|&x| x == 0.);
                    if !empty {
                        ptr::copy_nonoverlapping(buf.as_ptr(), o.pBuffer as *mut f32, n);
                    }
                    for (m, db) in shared.levels.iter().zip(a.stream.levels) {
                        m.store(db, Ordering::Relaxed);
                    }
                    shared.callbacks.fetch_add(1, Ordering::Release);
                    o.u32ValidFrameCount = i.u32ValidFrameCount;
                    o.u32BufferFlags = if empty { BUFFER_SILENT } else { BUFFER_VALID };
                    return;
                }
                shared.faults.fetch_add(1, Ordering::Release);
                a.failed = true;
            }
            // Sans moteur, ou après une erreur de calcul, la source est encore intacte.
            if !silent && n > 0 {
                ptr::copy(i.pBuffer as *const f32, o.pBuffer as *mut f32, n);
            }
            o.u32ValidFrameCount = i.u32ValidFrameCount;
            o.u32BufferFlags = i.u32BufferFlags;
        }
    }
}
