//! Small Windows-only helpers: start with Windows, and set the default microphone.

use std::ffi::c_void;
use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Media::Audio::{eCapture, eConsole, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>("Hush"))
        .is_ok()
}

pub fn set_autostart(on: bool) {
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY) else {
        return;
    };
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let _ = key.set_value("Hush", &format!("\"{}\" --hidden", exe.display()));
        }
    } else {
        let _ = key.delete_value("Hush");
    }
}

/// Returns false if another copy of Hush is already running.
pub fn single_instance() -> bool {
    let name: Vec<u16> = "Local\\HushNoiseSuppressor\0".encode_utf16().collect();
    unsafe {
        let _ = CreateMutexW(None, true, PCWSTR(name.as_ptr()));
        GetLastError().0 != 183 // ERROR_ALREADY_EXISTS
    }
}

// Windows' (undocumented but long-stable) policy config interface, the same one
// the Sound control panel uses to change default devices.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn get_mix_format(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn get_device_format(&self, a: PCWSTR, b: i32, c: *mut c_void) -> HRESULT;
    fn reset_device_format(&self, a: PCWSTR) -> HRESULT;
    fn set_device_format(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    fn get_processing_period(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> HRESULT;
    fn set_processing_period(&self, a: PCWSTR, b: *mut i64) -> HRESULT;
    fn get_share_mode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn set_share_mode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn get_property_value(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
    fn set_property_value(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
    fn set_default_endpoint(&self, id: PCWSTR, role: i32) -> HRESULT;
    fn set_endpoint_visibility(&self, a: PCWSTR, b: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// Make the given endpoint the Windows default (for apps and for calls).
pub fn set_default_endpoint(endpoint_id: &str) -> bool {
    let wide: Vec<u16> = endpoint_id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(cfg) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else {
            return false;
        };
        // 0 = console, 1 = multimedia, 2 = communications
        (0..3).all(|role| cfg.set_default_endpoint(PCWSTR(wide.as_ptr()), role).is_ok())
    }
}

fn default_id(flow: EDataFlow) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
        let id = dev.GetId().ok()?;
        let out = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const c_void));
        out
    }
}

pub fn default_mic_id() -> Option<String> {
    default_id(eCapture)
}

pub fn default_speaker_id() -> Option<String> {
    default_id(eRender)
}
