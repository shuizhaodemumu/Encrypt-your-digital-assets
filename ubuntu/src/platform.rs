use crate::core::*;
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use anyhow::{ensure, Context, Result};
use rand::rngs::OsRng;
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey},
    Oaep, RsaPrivateKey,
};
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os("ENCRYPT_ASSETS_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return p;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("encrypt-assets-ubuntu")
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && m.uid() == unsafe { libc::geteuid() },
        "数据目录不属于当前用户或是符号链接"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    fs::rename(src, dst).context("保存文件失败")?;
    if let Some(parent) = dst.parent().filter(|p| !p.as_os_str().is_empty()) {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub struct ViewerInstance(File);
fn lock_file(path: &Path, wait: bool) -> Result<Option<ViewerInstance>> {
    if let Some(p) = path.parent() {
        private_dir(p)?;
    }
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let result = unsafe {
        libc::flock(
            f.as_raw_fd(),
            libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB },
        )
    };
    if result != 0 {
        let e = std::io::Error::last_os_error();
        if !wait && e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        return Err(e.into());
    }
    Ok(Some(ViewerInstance(f)))
}
impl Drop for ViewerInstance {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
pub fn viewer_instance(info: &PackageInfo) -> Result<Option<ViewerInstance>> {
    lock_file(
        &root()
            .join("locks")
            .join(format!("{}.lock", info.content_id)),
        false,
    )
}
fn read_private(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    use std::io::Read;
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let m = f.metadata()?;
    ensure!(
        m.is_file()
            && m.uid() == unsafe { libc::geteuid() }
            && m.permissions().mode() & 0o077 == 0
            && m.len() <= 8 * 1024 * 1024,
        "本地保护文件权限或长度无效"
    );
    let mut b = Zeroizing::new(Vec::new());
    f.read_to_end(&mut b)?;
    Ok(b)
}
fn master() -> Result<Zeroizing<Vec<u8>>> {
    private_dir(&root())?;
    let _lock = lock_file(&root().join("master.lock"), true)?;
    let p = root().join("local-protection.key");
    if !p.exists() {
        atomic(&p, &random::<32>())?;
    }
    let key = read_private(&p)?;
    ensure!(key.len() == 32, "本地保护密钥损坏");
    Ok(key)
}
pub fn local_protect(data: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(data.len() <= 8 * 1024 * 1024, "本地状态过大");
    let k = master()?;
    let aes = Aes256Gcm::new_from_slice(&k)?;
    if protect {
        let nonce = random::<12>();
        let mut out = b"OLINUX01".to_vec();
        out.extend(nonce);
        out.extend(
            aes.encrypt(
                (&nonce).into(),
                aes_gcm::aead::Payload {
                    msg: data,
                    aad: b"OLINUX01",
                },
            )
            .map_err(|_| anyhow::anyhow!("本地状态加密失败"))?,
        );
        Ok(Zeroizing::new(out))
    } else {
        ensure!(
            data.len() >= 36 && &data[..8] == b"OLINUX01",
            "本地状态格式损坏"
        );
        Ok(Zeroizing::new(
            aes.decrypt(
                data[8..20].into(),
                aes_gcm::aead::Payload {
                    msg: &data[20..],
                    aad: b"OLINUX01",
                },
            )
            .map_err(|_| anyhow::anyhow!("本地状态损坏或不属于此用户数据目录"))?,
        ))
    }
}
pub fn protected_write(path: &Path, b: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        private_dir(p)?;
    }
    atomic(path, &local_protect(b, true)?)
}
pub fn uuid() -> Result<String> {
    if let Ok(v) = fs::read_to_string("/sys/class/dmi/id/product_uuid") {
        if device_id(v.trim()).is_ok() {
            return Ok(v.trim().into());
        }
    }
    let v = fs::read_to_string("/etc/machine-id").context("无法读取 DMI UUID 和系统 machine-id")?;
    let v = v.trim();
    ensure!(
        v.len() == 32 && v.bytes().all(|c| c.is_ascii_hexdigit()),
        "machine-id 无效"
    );
    Ok(uuid::Uuid::parse_str(v)?.to_string())
}
pub struct Device {
    pub id: String,
    pub hardware: Option<crate::hardware::Hardware>,
    key: RsaPrivateKey,
}
impl Device {
    pub fn load() -> Result<Self> {
        private_dir(&root())?;
        let _lock = lock_file(&root().join("device.lock"), true)?;
        let id = device_id(&uuid()?)?;
        let path = root().join("device.key");
        let key = if path.exists() {
            RsaPrivateKey::from_pkcs8_der(&local_protect(&read_private(&path)?, false)?)?
        } else {
            let key = RsaPrivateKey::new(&mut OsRng, 3072)?;
            protected_write(&path, key.to_pkcs8_der()?.as_bytes())?;
            key
        };
        Ok(Self {
            id,
            key,
            hardware: None,
        })
    }
    pub fn for_info(info: &PackageInfo) -> Result<Self> {
        let mut d = Self::load()?;
        if info.version == 2 {
            let h = crate::hardware::Hardware::collect()?;
            d.id = h.id(&info.hardware_policy)?;
            d.hardware = Some(h);
        }
        Ok(d)
    }
    #[cfg(test)]
    pub fn with_hardware(h: crate::hardware::Hardware, policy: &str) -> Result<Self> {
        let mut d = Self::load()?;
        d.id = h.id(policy)?;
        d.hardware = Some(h);
        Ok(d)
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
                .context("内容密钥解封失败")?,
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
    decode(&local_protect(&read_private(&p)?, false)?, 8 * 1024 * 1024)
}
pub fn save_state(id: uuid::Uuid, s: &ActivationState) -> Result<()> {
    protected_write(&state_path(id), &encode(s)?)
}
