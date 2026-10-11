use crate::core::*;
use anyhow::{ensure, Context, Result};
use rand::rngs::OsRng;
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey},
    Oaep, RsaPrivateKey,
};
use std::{
    fs,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::*,
    Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING},
    System::SystemInformation::GetSystemFirmwareTable,
};
use zeroize::Zeroizing;
#[cfg(feature = "office-state-compat")]
pub const STATE_NAMESPACE: &str = "OfflineLicenseOfficeRust";
#[cfg(not(feature = "office-state-compat"))]
pub const STATE_NAMESPACE: &str = "OfflineLicenseBilingualRust";
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(STATE_NAMESPACE)
}
pub fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    let a = wide(&src.to_string_lossy().replace('/', "\\"));
    let b = wide(&dst.to_string_lossy().replace('/', "\\"));
    ensure!(
        unsafe {
            MoveFileExW(
                a.as_ptr(),
                b.as_ptr(),
                MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_COPY_ALLOWED,
            )
        } != 0,
        crate::tr_format!(
            "保存文件失败（{} -> {}）：{}",
            "Could not save file ({} -> {}): {}",
            src.display(),
            dst.display(),
            std::io::Error::last_os_error()
        )
    );
    Ok(())
}
pub fn dpapi(data: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(
        data.len() <= 8 * 1024 * 1024,
        crate::i18n::text("保护数据过大")
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
    };
    ensure!(
        ok != 0,
        crate::i18n::text("本地保护状态损坏或不属于当前 Windows 用户")
    );
    let raw = unsafe { std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize) };
    let b = Zeroizing::new(raw.to_vec());
    raw.fill(0);
    unsafe { LocalFree(out.pbData as *mut std::ffi::c_void) };
    Ok(b)
}
pub fn protected_write(path: &Path, b: &[u8]) -> Result<()> {
    atomic(path, &dpapi(b, true)?)
}
pub fn uuid() -> Result<String> {
    unsafe {
        let signature = u32::from_be_bytes(*b"RSMB");
        let len = GetSystemFirmwareTable(signature, 0, ptr::null_mut(), 0);
        ensure!(
            len >= 8 && len < 1024 * 1024,
            crate::i18n::text("无法读取 SMBIOS 硬件 UUID")
        );
        let mut b = vec![0; len as usize];
        ensure!(
            GetSystemFirmwareTable(signature, 0, b.as_mut_ptr() as _, len) == len,
            crate::i18n::text("硬件信息读取失败")
        );
        let modern = b[1] > 2 || (b[1] == 2 && b[2] >= 6);
        let mut at = 8usize;
        while at + 4 <= b.len() {
            let kind = b[at];
            let size = b[at + 1] as usize;
            ensure!(
                size >= 4 && at + size <= b.len(),
                crate::i18n::text("SMBIOS 数据损坏")
            );
            if kind == 1 && size >= 24 {
                let bytes: [u8; 16] = b[at + 8..at + 24].try_into()?;
                let id = if modern {
                    uuid::Uuid::from_bytes_le(bytes)
                } else {
                    uuid::Uuid::from_bytes(bytes)
                };
                return Ok(id.to_string());
            }
            at += size;
            while at + 1 < b.len() && (b[at] != 0 || b[at + 1] != 0) {
                at += 1;
            }
            at += 2;
            if kind == 127 {
                break;
            }
        }
    }
    anyhow::bail!(crate::i18n::text("主板 UUID 不可用"))
}
pub struct Device {
    pub id: String,
    pub hardware: Option<crate::hardware::Hardware>,
    key: RsaPrivateKey,
}
impl Device {
    #[cfg(test)]
    pub fn with_hardware(hardware: crate::hardware::Hardware, policy: &str) -> Result<Self> {
        let mut device = Self::load()?;
        device.id = hardware.id(policy)?;
        device.hardware = Some(hardware);
        Ok(device)
    }
    pub fn load() -> Result<Self> {
        let _key_lock = device_key_lock()?;
        let id = device_id(&uuid()?)?;
        let path = root().join("device.key");
        let key = if path.exists() {
            RsaPrivateKey::from_pkcs8_der(&dpapi(&fs::read(path)?, false)?)?
        } else {
            let k = RsaPrivateKey::new(&mut OsRng, 3072)?;
            let raw = k.to_pkcs8_der()?;
            protected_write(&path, raw.as_bytes())?;
            k
        };
        Ok(Self {
            id,
            key,
            hardware: None,
        })
    }
    pub fn for_info(info: &PackageInfo) -> Result<Self> {
        let mut device = Self::load()?;
        if info.version == 2 {
            let hardware = crate::hardware::Hardware::collect()?;
            device.id = hardware.id(&info.hardware_policy)?;
            device.hardware = Some(hardware);
        }
        Ok(device)
    }
    pub fn request(&self) -> Result<DeviceRequest> {
        Ok(DeviceRequest {
            version: if self.hardware.is_some() { 2 } else { 1 },
            device_id: self.id.clone(),
            rsa_public_key: b64(self.key.to_public_key().to_public_key_der()?.as_bytes()),
        })
    }
    pub fn unwrap(&self, s: &str) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(
            self.key
                .decrypt(Oaep::new::<sha2::Sha256>(), &unb64(s)?)
                .context(crate::i18n::text("内容密钥解封失败"))?,
        ))
    }
}
pub fn state_path(id: uuid::Uuid) -> PathBuf {
    root().join("state").join(format!("{}.dat", id.simple()))
}
pub fn load_state(id: uuid::Uuid) -> Result<ActivationState> {
    let p = state_path(id);
    if !p.exists() {
        return Ok(ActivationState::default());
    }
    decode(&dpapi(&fs::read(p)?, false)?, 8 * 1024 * 1024)
}
pub fn save_state(id: uuid::Uuid, s: &ActivationState) -> Result<()> {
    protected_write(&state_path(id), &encode(s)?)
}
pub struct ViewerInstance(isize);
impl Drop for ViewerInstance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
fn device_key_lock() -> Result<ViewerInstance> {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    unsafe {
        let name = format!(
            "Local\\{}-DeviceKey-{}",
            STATE_NAMESPACE,
            fingerprint(root().to_string_lossy().as_bytes())
        );
        let handle = CreateMutexW(ptr::null(), 0, wide(&name).as_ptr());
        ensure!(handle != 0, crate::i18n::text("无法锁定设备密钥存储"));
        let waited = WaitForSingleObject(handle, 30000);
        if waited != 0 && waited != 0x80 {
            CloseHandle(handle);
            anyhow::bail!(crate::i18n::text("设备密钥正在初始化，请稍后再试"));
        }
        Ok(ViewerInstance(handle))
    }
}
pub fn viewer_instance(info: &PackageInfo) -> Result<Option<ViewerInstance>> {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    unsafe {
        let handle = CreateMutexW(
            ptr::null(),
            1,
            wide(&format!(
                "Local\\{}-{}",
                STATE_NAMESPACE,
                info.content_id.simple()
            ))
            .as_ptr(),
        );
        ensure!(handle != 0, crate::i18n::text("无法创建内容单实例锁"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(ViewerInstance(handle)))
    }
}
