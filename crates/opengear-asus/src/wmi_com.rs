//! Native Win32 COM WMI client (`IWbemServices`) and System32 helpers.
//!
//! Replaces slow `powershell.exe` WMI child processes with direct in-process COM calls.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) enum WmiVal {
    Null,
    String(String),
    U8(u8),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    Bytes(Vec<u8>),
    Ints(Vec<i64>),
    Object(BTreeMap<String, WmiVal>),
}

impl WmiVal {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(b) => Some(b),
            Self::U8(v) => Some(v != 0),
            Self::U32(v) => Some(v != 0),
            Self::I64(v) => Some(v != 0),
            Self::U64(v) => Some(v != 0),
            _ => None,
        }
    }

    pub(crate) fn as_u64(&self) -> Option<u64> {
        match *self {
            Self::U8(v) => Some(v as u64),
            Self::U32(v) => Some(v as u64),
            Self::U64(v) => Some(v),
            Self::I64(v) if v >= 0 => Some(v as u64),
            Self::String(ref s) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::U8(v) => Some(v as i64),
            Self::U32(v) => Some(v as i64),
            Self::I64(v) => Some(v),
            Self::U64(v) => i64::try_from(v).ok(),
            Self::String(ref s) => s.trim().parse::<i64>().ok(),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::F64(v) => Some(v),
            Self::U8(v) => Some(v as f64),
            Self::U32(v) => Some(v as f64),
            Self::I64(v) => Some(v as f64),
            Self::U64(v) => Some(v as f64),
            Self::String(ref s) => s.trim().replace(',', ".").parse::<f64>().ok(),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    pub(crate) fn as_ints(&self) -> Option<&[i64]> {
        match self {
            Self::Ints(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub(crate) fn as_object(&self) -> Option<&BTreeMap<String, WmiVal>> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }
}

/// Resolve an executable or DLL inside `%SystemRoot%\System32` (never searching `PATH` or CWD).
#[cfg(windows)]
pub(crate) fn system32_path(file: &str) -> PathBuf {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = [0u16; 260];
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
    let base = if len > 0 && (len as usize) < buf.len() {
        PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]))
    } else {
        PathBuf::from(r"C:\Windows\System32")
    };
    base.join(file)
}

#[cfg(windows)]
pub(crate) fn load_system32_library(dll_name: &str) -> Result<libloading::Library> {
    let full = system32_path(dll_name);
    unsafe { libloading::Library::new(&full) }
        .with_context(|| format!("Impossible de charger {}", full.display()))
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE, SAFEARRAY,
    };
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayCreateVector, SafeArrayDestroy, SafeArrayGetLBound,
        SafeArrayGetUBound, SafeArrayUnaccessData,
    };
    use windows::Win32::System::Variant::{
        VARENUM, VARIANT, VT_ARRAY, VT_BOOL, VT_BSTR, VT_EMPTY, VT_I1, VT_I2, VT_I4, VT_I8,
        VT_NULL, VT_R4, VT_R8, VT_UI1, VT_UI2, VT_UI4, VT_UI8, VT_UNKNOWN, VariantClear,
    };
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
        WBEM_FLAG_NONSYSTEM_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
        WBEM_INFINITE, WbemLocator,
    };
    use windows::core::{BSTR, IUnknown, Interface, PCWSTR};

    const RPC_C_AUTHN_WINNT: u32 = 10;
    const RPC_C_AUTHZ_NONE: u32 = 0;

    pub(crate) struct ComGuard {
        uninit: bool,
    }

    impl ComGuard {
        pub(crate) fn new() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr == S_OK || hr == S_FALSE {
                Ok(Self { uninit: true })
            } else if hr == RPC_E_CHANGED_MODE {
                Ok(Self { uninit: false })
            } else {
                Err(anyhow::anyhow!("CoInitializeEx failed: 0x{:08X}", hr.0 as u32))
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    pub(crate) struct WmiConn {
        // Declared BEFORE `_com` so `IWbemServices::Release()` runs prior to `CoUninitialize()`.
        services: IWbemServices,
        _com: ComGuard,
    }

    impl WmiConn {
        pub(crate) fn connect(namespace: &str) -> Result<Self> {
            let com = ComGuard::new()?;
            let locator: IWbemLocator =
                unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
                    .context("CoCreateInstance(WbemLocator)")?;
            let ns = BSTR::from(namespace);
            let services = unsafe {
                locator.ConnectServer(&ns, &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
            }
            .with_context(|| format!("ConnectServer({namespace})"))?;

            unsafe {
                CoSetProxyBlanket(
                    &services,
                    RPC_C_AUTHN_WINNT,
                    RPC_C_AUTHZ_NONE,
                    None,
                    RPC_C_AUTHN_LEVEL_CALL,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                )
            }
            .context("CoSetProxyBlanket")?;

            Ok(Self { services, _com: com })
        }

        pub(crate) fn query(&self, wql: &str) -> Result<Vec<BTreeMap<String, WmiVal>>> {
            let lang = BSTR::from("WQL");
            let q = BSTR::from(wql);
            let enumerator = unsafe {
                self.services.ExecQuery(
                    &lang,
                    &q,
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
            }
            .with_context(|| format!("ExecQuery({wql})"))?;

            let mut rows = Vec::new();
            loop {
                let mut objs = [None; 1];
                let mut returned = 0u32;
                let hr = unsafe { enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned) };
                if hr.is_err() || returned == 0 {
                    break;
                }
                let Some(obj) = objs[0].take() else {
                    break;
                };
                rows.push(read_object_props(&obj)?);
            }
            Ok(rows)
        }

        fn instance_path_by_wql(&self, wql: &str) -> Result<Option<String>> {
            let lang = BSTR::from("WQL");
            let q = BSTR::from(wql);
            let enumerator = unsafe {
                self.services.ExecQuery(
                    &lang,
                    &q,
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
            }?;
            let mut objs = [None; 1];
            let mut returned = 0u32;
            let hr = unsafe { enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned) };
            if hr.is_err() || returned == 0 {
                return Ok(None);
            }
            let Some(obj) = objs[0].take() else {
                return Ok(None);
            };
            let mut val = VARIANT::default();
            let path_key = BSTR::from("__PATH");
            let get_res = unsafe { obj.Get(PCWSTR(path_key.as_ptr()), 0, &mut val, None, None) };
            let wv = variant_to_wmi(&val);
            unsafe {
                let _ = VariantClear(&mut val);
            }
            get_res?;
            Ok(wv.as_str().map(|s| s.to_string()))
        }

        pub(crate) fn first_instance_path(&self, class_name: &str) -> Result<Option<String>> {
            self.instance_path_by_wql(&format!("SELECT * FROM {class_name}"))
        }

        pub(crate) fn first_active_instance_path(&self, class_name: &str) -> Result<Option<String>> {
            if let Ok(Some(path)) =
                self.instance_path_by_wql(&format!("SELECT * FROM {class_name} WHERE Active = TRUE"))
            {
                return Ok(Some(path));
            }
            self.first_instance_path(class_name)
        }

        pub(crate) fn exec_method(
            &self,
            object_path: &str,
            class_name: &str,
            method_name: &str,
            params: &[(&str, WmiVal)],
        ) -> Result<BTreeMap<String, WmiVal>> {
            let mut cls_obj: Option<IWbemClassObject> = None;
            let cls_bstr = BSTR::from(class_name);
            unsafe {
                self.services.GetObject(
                    &cls_bstr,
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut cls_obj),
                    None,
                )
            }
            .with_context(|| format!("GetObject({class_name})"))?;
            let cls_obj = cls_obj.context("WMI class object missing")?;

            let meth_bstr = BSTR::from(method_name);
            let mut in_sig: Option<IWbemClassObject> = None;
            unsafe {
                cls_obj.GetMethod(
                    PCWSTR(meth_bstr.as_ptr()),
                    0,
                    &mut in_sig,
                    std::ptr::null_mut(),
                )
            }
            .with_context(|| format!("GetMethod({method_name})"))?;

            let in_inst = if let Some(sig) = in_sig {
                let inst = unsafe { sig.SpawnInstance(0) }?;
                for (name, val) in params {
                    let key = BSTR::from(*name);
                    let mut v = wmi_to_variant(val);
                    let put_res = unsafe { inst.Put(PCWSTR(key.as_ptr()), 0, &v, 0) };
                    unsafe {
                        let _ = VariantClear(&mut v);
                    }
                    put_res.with_context(|| format!("Put param {name}"))?;
                }
                Some(inst)
            } else {
                None
            };

            let obj_bstr = BSTR::from(object_path);
            let mut out_obj: Option<IWbemClassObject> = None;
            unsafe {
                self.services.ExecMethod(
                    &obj_bstr,
                    &meth_bstr,
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    in_inst.as_ref(),
                    Some(&mut out_obj),
                    None,
                )
            }
            .with_context(|| format!("ExecMethod({method_name})"))?;

            match out_obj {
                Some(o) => read_object_props(&o),
                None => Ok(BTreeMap::new()),
            }
        }

        /// Execute an HP BIOS WMI command via `hpqBIntM::<method_name>` with a `hpqBDataIn` packet.
        pub(crate) fn exec_hpq_bios_int(
            &self,
            method_name: &str,
            command: u32,
            command_type: u32,
            data: &[u8],
        ) -> Result<(u32, Vec<u8>)> {
            let object_path = self
                .first_instance_path("hpqBIntM")?
                .context("Instance hpqBIntM introuvable dans root\\wmi")?;

            // 1. Spawn hpqBDataIn embedded object
            let mut data_cls: Option<IWbemClassObject> = None;
            let data_cls_bstr = BSTR::from("hpqBDataIn");
            unsafe {
                self.services.GetObject(
                    &data_cls_bstr,
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut data_cls),
                    None,
                )
            }
            .context("GetObject(hpqBDataIn)")?;
            let data_cls = data_cls.context("Classe hpqBDataIn absente")?;
            let in_data = unsafe { data_cls.SpawnInstance(0) }.context("SpawnInstance(hpqBDataIn)")?;

            put_variant(&in_data, "Sign", bytes_to_variant(b"SECU")?)?;
            put_variant(&in_data, "Command", VARIANT::from(command as i32))?;
            put_variant(&in_data, "CommandType", VARIANT::from(command_type as i32))?;
            put_variant(&in_data, "Size", VARIANT::from(data.len() as i32))?;
            put_variant(&in_data, "hpqBData", bytes_to_variant(data)?)?;

            // 2. Spawn InParameters for hpqBIntM::<method_name>
            let mut intm_cls: Option<IWbemClassObject> = None;
            let intm_cls_bstr = BSTR::from("hpqBIntM");
            unsafe {
                self.services.GetObject(
                    &intm_cls_bstr,
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut intm_cls),
                    None,
                )
            }
            .context("GetObject(hpqBIntM)")?;
            let intm_cls = intm_cls.context("Classe hpqBIntM absente")?;

            let meth_bstr = BSTR::from(method_name);
            let mut in_sig: Option<IWbemClassObject> = None;
            unsafe {
                intm_cls.GetMethod(
                    PCWSTR(meth_bstr.as_ptr()),
                    0,
                    &mut in_sig,
                    std::ptr::null_mut(),
                )
            }
            .with_context(|| format!("GetMethod({method_name})"))?;
            let in_sig = in_sig.context("InParameters manquants sur hpqBIntM")?;
            let in_params = unsafe { in_sig.SpawnInstance(0) }?;

            let unk: IUnknown = in_data.cast().context("Cast hpqBDataIn -> IUnknown")?;
            put_variant(&in_params, "InData", unknown_to_variant(unk))?;

            let obj_bstr = BSTR::from(object_path.as_str());
            let mut out_obj: Option<IWbemClassObject> = None;
            unsafe {
                self.services.ExecMethod(
                    &obj_bstr,
                    &meth_bstr,
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&in_params),
                    Some(&mut out_obj),
                    None,
                )
            }
            .with_context(|| format!("ExecMethod({method_name})"))?;

            let out_obj = out_obj.context("Aucun résultat OutData renvoyé par hpqBIntM")?;
            let props = read_object_props(&out_obj)?;
            let out_data = props
                .get("OutData")
                .and_then(|v| v.as_object())
                .context("Propriété OutData absente ou non-objet")?;
            let rw_rc = out_data
                .get("rwReturnCode")
                .and_then(|v| v.as_u64())
                .unwrap_or(u32::MAX as u64) as u32;
            let bytes = out_data
                .get("Data")
                .and_then(|v| v.as_bytes())
                .map(|b| b.to_vec())
                .unwrap_or_default();
            Ok((rw_rc, bytes))
        }
    }

    fn put_variant(obj: &IWbemClassObject, name: &str, mut val: VARIANT) -> Result<()> {
        let key = BSTR::from(name);
        let res = unsafe { obj.Put(PCWSTR(key.as_ptr()), 0, &val, 0) };
        unsafe {
            let _ = VariantClear(&mut val);
        }
        res.with_context(|| format!("Put({name})"))
    }

    fn bytes_to_variant(bytes: &[u8]) -> Result<VARIANT> {
        unsafe {
            let psa = SafeArrayCreateVector(VT_UI1, 0, bytes.len() as u32);
            anyhow::ensure!(!psa.is_null(), "SafeArrayCreateVector(VT_UI1) failed");
            if !bytes.is_empty() {
                let mut ptr = std::ptr::null_mut();
                SafeArrayAccessData(psa, &mut ptr).map_err(|e| {
                    let _ = SafeArrayDestroy(psa);
                    anyhow::anyhow!("SafeArrayAccessData failed: {e}")
                })?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
                let _ = SafeArrayUnaccessData(psa);
            }
            let mut v = VARIANT::default();
            let raw = &mut v.Anonymous.Anonymous;
            raw.vt = VARENUM(VT_ARRAY.0 | VT_UI1.0);
            raw.Anonymous.parray = psa;
            Ok(v)
        }
    }

    fn unknown_to_variant(unk: IUnknown) -> VARIANT {
        let mut v = VARIANT::default();
        unsafe {
            let raw = &mut v.Anonymous.Anonymous;
            raw.vt = VT_UNKNOWN;
            raw.Anonymous.punkVal = std::mem::ManuallyDrop::new(Some(unk));
        }
        v
    }

    unsafe fn read_safearray_bytes(psa: *const SAFEARRAY) -> Option<Vec<u8>> {
        if psa.is_null() {
            return None;
        }
        let lb = unsafe { SafeArrayGetLBound(psa, 1).ok()? };
        let ub = unsafe { SafeArrayGetUBound(psa, 1).ok()? };
        if ub < lb {
            return Some(Vec::new());
        }
        let count = (ub - lb + 1) as usize;
        let mut ptr = std::ptr::null_mut();
        unsafe { SafeArrayAccessData(psa, &mut ptr).ok()? };
        let mut out = vec![0u8; count];
        unsafe {
            std::ptr::copy_nonoverlapping(ptr as *const u8, out.as_mut_ptr(), count);
            let _ = SafeArrayUnaccessData(psa);
        }
        Some(out)
    }

    unsafe fn read_safearray_ints<T: Copy + Into<i64>>(psa: *const SAFEARRAY) -> Option<Vec<i64>> {
        if psa.is_null() {
            return None;
        }
        let lb = unsafe { SafeArrayGetLBound(psa, 1).ok()? };
        let ub = unsafe { SafeArrayGetUBound(psa, 1).ok()? };
        if ub < lb {
            return Some(Vec::new());
        }
        let count = (ub - lb + 1) as usize;
        let mut ptr = std::ptr::null_mut();
        unsafe { SafeArrayAccessData(psa, &mut ptr).ok()? };
        let slice = unsafe { std::slice::from_raw_parts(ptr as *const T, count) };
        let out = slice.iter().map(|&x| x.into()).collect();
        unsafe {
            let _ = SafeArrayUnaccessData(psa);
        }
        Some(out)
    }

    fn read_object_props(obj: &IWbemClassObject) -> Result<BTreeMap<String, WmiVal>> {
        let mut map = BTreeMap::new();
        unsafe { obj.BeginEnumeration(WBEM_FLAG_NONSYSTEM_ONLY.0) }?;
        loop {
            let mut name = BSTR::new();
            let mut val = VARIANT::default();
            let hr = unsafe {
                obj.Next(
                    0,
                    &mut name,
                    &mut val,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if hr.is_err() {
                unsafe {
                    let _ = VariantClear(&mut val);
                }
                break;
            }
            let key = name.to_string();
            if key.is_empty() {
                unsafe {
                    let _ = VariantClear(&mut val);
                }
                break;
            }
            let wv = variant_to_wmi(&val);
            unsafe {
                let _ = VariantClear(&mut val);
            }
            map.insert(key, wv);
        }
        unsafe {
            let _ = obj.EndEnumeration();
        }
        Ok(map)
    }

    fn variant_to_wmi(v: &VARIANT) -> WmiVal {
        unsafe {
            let raw = &v.Anonymous.Anonymous;
            let vt = raw.vt;
            if (vt.0 & VT_ARRAY.0) != 0 {
                let base = VARENUM(vt.0 & 0x0FFF);
                let psa = raw.Anonymous.parray as *const SAFEARRAY;
                if base == VT_UI1 {
                    return read_safearray_bytes(psa)
                        .map(WmiVal::Bytes)
                        .unwrap_or(WmiVal::Null);
                } else if base == VT_I2 {
                    return read_safearray_ints::<i16>(psa)
                        .map(WmiVal::Ints)
                        .unwrap_or(WmiVal::Null);
                } else if base == VT_UI2 {
                    return read_safearray_ints::<u16>(psa)
                        .map(WmiVal::Ints)
                        .unwrap_or(WmiVal::Null);
                } else if base == VT_I4 {
                    return read_safearray_ints::<i32>(psa)
                        .map(WmiVal::Ints)
                        .unwrap_or(WmiVal::Null);
                } else if base == VT_UI4 {
                    return read_safearray_ints::<u32>(psa)
                        .map(WmiVal::Ints)
                        .unwrap_or(WmiVal::Null);
                }
                return WmiVal::Null;
            }
            if vt == VT_NULL || vt == VT_EMPTY {
                WmiVal::Null
            } else if vt == VT_UNKNOWN {
                if let Some(unk) = raw.Anonymous.punkVal.as_ref() {
                    if let Ok(obj) = unk.cast::<IWbemClassObject>() {
                        if let Ok(map) = read_object_props(&obj) {
                            return WmiVal::Object(map);
                        }
                    }
                }
                WmiVal::Null
            } else if vt == VT_BSTR {
                WmiVal::String(raw.Anonymous.bstrVal.to_string())
            } else if vt == VT_BOOL {
                WmiVal::Bool(raw.Anonymous.boolVal.as_bool())
            } else if vt == VT_UI1 {
                WmiVal::U8(raw.Anonymous.bVal)
            } else if vt == VT_I1 {
                WmiVal::I64(raw.Anonymous.cVal as i64)
            } else if vt == VT_I2 {
                WmiVal::I64(raw.Anonymous.iVal as i64)
            } else if vt == VT_UI2 {
                WmiVal::U32(raw.Anonymous.uiVal as u32)
            } else if vt == VT_I4 {
                WmiVal::I64(raw.Anonymous.lVal as i64)
            } else if vt == VT_UI4 {
                WmiVal::U32(raw.Anonymous.ulVal)
            } else if vt == VT_I8 {
                WmiVal::I64(raw.Anonymous.llVal)
            } else if vt == VT_UI8 {
                WmiVal::U64(raw.Anonymous.ullVal)
            } else if vt == VT_R4 {
                WmiVal::F64(raw.Anonymous.fltVal as f64)
            } else if vt == VT_R8 {
                WmiVal::F64(raw.Anonymous.dblVal)
            } else {
                WmiVal::Null
            }
        }
    }

    fn wmi_to_variant(val: &WmiVal) -> VARIANT {
        match val {
            WmiVal::Null | WmiVal::Bytes(_) | WmiVal::Ints(_) | WmiVal::Object(_) => {
                VARIANT::default()
            }
            WmiVal::String(s) => VARIANT::from(BSTR::from(s.as_str())),
            WmiVal::U8(v) => VARIANT::from(*v),
            // WMI COM (`IWbemClassObject::Put`) rejects VT_UI4 for CIM_UINT32 and expects VT_I4,
            // and requires VT_BSTR for 64-bit integer types (CIM_SINT64 / CIM_UINT64).
            WmiVal::U32(v) => VARIANT::from(*v as i32),
            WmiVal::I64(v) => VARIANT::from(BSTR::from(v.to_string())),
            WmiVal::U64(v) => VARIANT::from(BSTR::from(v.to_string())),
            WmiVal::F64(v) => VARIANT::from(*v),
            WmiVal::Bool(v) => VARIANT::from(*v),
        }
    }
}

#[cfg(windows)]
pub(crate) use imp::WmiConn;
