//! Le moteur audio peut agréger l'effet. Il conserve l'IUnknown interne et contrôle
//! l'identité ainsi que le comptage des interfaces publiques. Aucune référence forte
//! vers cet objet extérieur, qui possède lui-même l'objet interne.
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{fence, AtomicU32, Ordering},
};
use windows::{
    core::{IUnknown, IUnknown_Vtbl, Interface, Result, GUID, HRESULT},
    Win32::{Foundation::*, Media::Audio::Apo::*},
};

type Raw = *mut c_void;
type InterfaceId = *const GUID;
type Out = *mut Raw;

#[repr(C)]
struct Header {
    vtable: *const c_void,
    outer: Raw,
}

#[repr(C)]
struct Face<I: Interface> {
    header: Header,
    target: I,
}
impl<I: Interface> Face<I> {
    fn new(table: &'static I::Vtable, outer: Raw, target: I) -> Self {
        Self {
            header: Header {
                vtable: ptr::from_ref(table).cast(),
                outer,
            },
            target,
        }
    }
    fn raw(&self) -> Raw {
        ptr::from_ref(self).cast_mut().cast()
    }
}

unsafe fn unknown(object: Raw) -> *const IUnknown_Vtbl {
    // Toute interface COM débute par le pointeur vers sa vtable, dont les trois
    // premières méthodes sont celles d'IUnknown. Le contrôleur vit pendant l'appel.
    *object.cast::<*const IUnknown_Vtbl>()
}

unsafe extern "system" fn query_outer(this: Raw, id: InterfaceId, out: Out) -> HRESULT {
    let outer = (*this.cast::<Header>()).outer;
    ((*unknown(outer)).QueryInterface)(outer, id, out)
}
unsafe extern "system" fn add_outer(this: Raw) -> u32 {
    let outer = (*this.cast::<Header>()).outer;
    ((*unknown(outer)).AddRef)(outer)
}
unsafe extern "system" fn release_outer(this: Raw) -> u32 {
    let outer = (*this.cast::<Header>()).outer;
    ((*unknown(outer)).Release)(outer)
}
const DELEGATE: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: query_outer,
    AddRef: add_outer,
    Release: release_outer,
};

// Les signatures sont vérifiées par les vtables du SDK. L'algorithme et les
// règles sur les buffers sont exactement ceux de l'instance COM autonome.
macro_rules! forward {
    ($interface:ty, $name:ident($($arg:ident : $kind:ty),*) -> $ret:ty) => {{
        unsafe extern "system" fn call(this: Raw, $($arg: $kind),*) -> $ret {
            let target = &(*this.cast::<Face<$interface>>()).target;
            (target.vtable().$name)(target.as_raw(), $($arg),*)
        }
        call
    }};
}
static AUDIO: IAudioProcessingObject_Vtbl = IAudioProcessingObject_Vtbl {
    base__: DELEGATE,
    Reset: forward!(IAudioProcessingObject, Reset() -> HRESULT),
    GetLatency: forward!(IAudioProcessingObject, GetLatency(t: *mut i64) -> HRESULT),
    GetRegistrationProperties: forward!(IAudioProcessingObject, GetRegistrationProperties(p: *mut *mut APO_REG_PROPERTIES) -> HRESULT),
    Initialize: forward!(IAudioProcessingObject, Initialize(n: u32, data: *const u8) -> HRESULT),
    IsInputFormatSupported: forward!(IAudioProcessingObject, IsInputFormatSupported(a: Raw, b: Raw, out: Out) -> HRESULT),
    IsOutputFormatSupported: forward!(IAudioProcessingObject, IsOutputFormatSupported(a: Raw, b: Raw, out: Out) -> HRESULT),
    GetInputChannelCount: forward!(IAudioProcessingObject, GetInputChannelCount(n: *mut u32) -> HRESULT),
};
static CONFIG: IAudioProcessingObjectConfiguration_Vtbl =
    IAudioProcessingObjectConfiguration_Vtbl {
        base__: DELEGATE,
        LockForProcess: forward!(IAudioProcessingObjectConfiguration, LockForProcess(ni: u32, i: *const *const APO_CONNECTION_DESCRIPTOR, no: u32, o: *const *const APO_CONNECTION_DESCRIPTOR) -> HRESULT),
        UnlockForProcess: forward!(IAudioProcessingObjectConfiguration, UnlockForProcess() -> HRESULT),
    };
static RT: IAudioProcessingObjectRT_Vtbl = IAudioProcessingObjectRT_Vtbl {
    base__: DELEGATE,
    APOProcess: forward!(IAudioProcessingObjectRT, APOProcess(ni: u32, i: *const *const APO_CONNECTION_PROPERTY, no: u32, o: *mut *mut APO_CONNECTION_PROPERTY) -> ()),
    CalcInputFrames: forward!(IAudioProcessingObjectRT, CalcInputFrames(n: u32) -> u32),
    CalcOutputFrames: forward!(IAudioProcessingObjectRT, CalcOutputFrames(n: u32) -> u32),
};
static EFFECTS: IAudioSystemEffects2_Vtbl = IAudioSystemEffects2_Vtbl {
    base__: IAudioSystemEffects_Vtbl { base__: DELEGATE },
    GetEffectsList: forward!(IAudioSystemEffects2, GetEffectsList(ids: *mut *mut GUID, n: *mut u32, event: HANDLE) -> HRESULT),
};

#[repr(C)]
struct Aggregate {
    inner: &'static IUnknown_Vtbl,
    refs: AtomicU32,
    audio: Face<IAudioProcessingObject>,
    config: Face<IAudioProcessingObjectConfiguration>,
    rt: Face<IAudioProcessingObjectRT>,
    effects: Face<IAudioSystemEffects2>,
}
unsafe extern "system" fn query_inner(this: Raw, id: InterfaceId, out: Out) -> HRESULT {
    if id.is_null() || out.is_null() {
        return E_POINTER;
    }
    *out = ptr::null_mut();
    if *id == IUnknown::IID {
        add_inner(this);
        *out = this;
        return S_OK;
    }
    let a = &*this.cast::<Aggregate>();
    let face = match *id {
        IAudioProcessingObject::IID => a.audio.raw(),
        IAudioProcessingObjectConfiguration::IID => a.config.raw(),
        IAudioProcessingObjectRT::IID => a.rt.raw(),
        IAudioSystemEffects::IID | IAudioSystemEffects2::IID => a.effects.raw(),
        _ => return E_NOINTERFACE,
    };
    add_outer(face);
    *out = face;
    S_OK
}
unsafe extern "system" fn add_inner(this: Raw) -> u32 {
    (*this.cast::<Aggregate>())
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}
unsafe extern "system" fn release_inner(this: Raw) -> u32 {
    let a = this.cast::<Aggregate>();
    let remaining = (*a).refs.fetch_sub(1, Ordering::Release) - 1;
    if remaining == 0 {
        fence(Ordering::Acquire);
        drop(Box::from_raw(a));
    }
    remaining
}
static INNER: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: query_inner,
    AddRef: add_inner,
    Release: release_inner,
};

/// La fabrique remet l'unique référence initiale de l'IUnknown interne à l'agrégateur.
pub(super) fn create(outer: &IUnknown, effect: IAudioProcessingObject) -> Result<Raw> {
    let outer = outer.as_raw();
    let a = Aggregate {
        inner: &INNER,
        refs: AtomicU32::new(1),
        config: Face::new(&CONFIG, outer, effect.cast()?),
        rt: Face::new(&RT, outer, effect.cast()?),
        effects: Face::new(&EFFECTS, outer, effect.cast()?),
        audio: Face::new(&AUDIO, outer, effect),
    };
    Ok(Box::into_raw(Box::new(a)).cast())
}
