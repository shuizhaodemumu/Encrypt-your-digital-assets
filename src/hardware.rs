use crate::{core::*, platform::Device};
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit,
};
use anyhow::{ensure, Context, Result};
use p256::{
    ecdh::diffie_hellman,
    pkcs8::{DecodePublicKey, EncodePublicKey},
    PublicKey, SecretKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;
pub const STABLE: &str = "stable-hardware-v2";
pub const STRICT: &str = "strict-hardware-v2";
pub type Row = BTreeMap<String, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Hardware {
    pub computer: Vec<Row>,
    pub base_board: Vec<Row>,
    pub computer_product: Vec<Row>,
    pub disks: Vec<Row>,
    pub processors: Vec<Row>,
    pub bios: Vec<Row>,
    pub memory: Vec<Row>,
    pub network_adapters: Vec<Row>,
}
impl Hardware {
    pub fn collect() -> Result<Self> {
        let encoded = b64(&include_str!("../assets/hardware.ps1")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>());
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &encoded,
            ])
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut b = Vec::new();
            stdout.take(262145).read_to_end(&mut b).map(|_| b)
        });
        let err = std::thread::spawn(move || {
            let mut b = Vec::new();
            stderr.take(65537).read_to_end(&mut b).map(|_| b)
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(25) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!(crate::i18n::text("硬件查询超时，请检查 Windows WMI 服务"));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let output = out
            .join()
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("硬件查询线程异常")))??;
        let errors = err
            .join()
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("硬件查询线程异常")))??;
        ensure!(
            status.success(),
            crate::tr_format!(
                "硬件查询失败：{}",
                "Hardware query failed: {}",
                String::from_utf8_lossy(&errors)
            )
        );
        let raw = output.strip_prefix(&[239, 187, 191]).unwrap_or(&output);
        let profile: Self = decode(raw, 262144).context(crate::i18n::text("硬件信息格式无效"))?;
        profile.validate()?;
        Ok(profile)
    }
    pub fn validate(&self) -> Result<()> {
        let groups = [
            (&self.computer, &["Manufacturer", "Model"][..]),
            (
                &self.base_board,
                &["Manufacturer", "Product", "SerialNumber"][..],
            ),
            (&self.computer_product, &["Name", "Vendor", "UUID"][..]),
            (
                &self.disks,
                &[
                    "Model",
                    "SerialNumber",
                    "InterfaceType",
                    "PNPDeviceID",
                    "BusType",
                ][..],
            ),
            (
                &self.processors,
                &["Name", "SerialNumber", "ProcessorId"][..],
            ),
            (&self.bios, &["SerialNumber"][..]),
            (&self.memory, &["DeviceLocator", "SerialNumber"][..]),
            (
                &self.network_adapters,
                &["Name", "MACAddress", "NetConnectionStatus"][..],
            ),
        ];
        for (rows, fields) in groups {
            ensure!(rows.len() <= 256, crate::i18n::text("硬件记录过多"));
            for row in rows {
                ensure!(
                    row.len() == fields.len() && fields.iter().all(|key| row.contains_key(*key)),
                    crate::i18n::text("硬件字段不完整或包含非硬件字段")
                );
                ensure!(
                    row.values().all(|value| value.len() <= 2048),
                    crate::i18n::text("硬件字段过长")
                );
            }
        }
        ensure!(
            !self.computer.is_empty() && !self.base_board.is_empty() && !self.processors.is_empty(),
            crate::i18n::text("关键硬件信息缺失")
        );
        ensure!(
            self.computer_product
                .iter()
                .any(|r| r.get("UUID").is_some_and(|s| device_id(s).is_ok())),
            crate::i18n::text("主板 UUID 无效")
        );
        Ok(())
    }
    pub fn id(&self, policy: &str) -> Result<String> {
        self.validate()?;
        ensure!(
            policy == STABLE || policy == STRICT,
            crate::i18n::text("不支持的硬件绑定策略")
        );
        let mut value = serde_json::to_value(self)?;
        if policy == STABLE {
            if let Some(disks) = value["Disks"].as_array_mut() {
                disks.retain(|row| {
                    row["BusType"].as_str().unwrap_or("") != "7"
                        && !row["InterfaceType"]
                            .as_str()
                            .unwrap_or("")
                            .eq_ignore_ascii_case("USB")
                        && !row["PNPDeviceID"]
                            .as_str()
                            .unwrap_or("")
                            .to_uppercase()
                            .contains("USB")
                });
                for row in disks {
                    if let Some(object) = row.as_object_mut() {
                        object.remove("BusType");
                        object.remove("InterfaceType");
                        object.remove("PNPDeviceID");
                    }
                }
            }
            if let Some(adapters) = value["NetworkAdapters"].as_array_mut() {
                for row in adapters {
                    if let Some(object) = row.as_object_mut() {
                        object.remove("NetConnectionStatus");
                    }
                }
            }
        }
        fn normalize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(s) => {
                    *s = s
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_uppercase()
                }
                serde_json::Value::Array(rows) => {
                    for r in rows.iter_mut() {
                        normalize(r)
                    }
                    rows.sort_by_key(|r| serde_json::to_string(r).unwrap());
                    rows.dedup();
                }
                serde_json::Value::Object(object) => {
                    for v in object.values_mut() {
                        normalize(v)
                    }
                }
                _ => {}
            }
        }
        normalize(&mut value);
        let mut encoded = policy.as_bytes().to_vec();
        encoded.extend(encode(&value)?);
        Ok(fingerprint(&encoded))
    }
    pub fn display(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Enrollment {
    pub version: i32,
    pub request_id: Uuid,
    pub content_id: Uuid,
    pub issuer_fingerprint: String,
    pub hardware_policy: String,
    pub created_at_utc_seconds: i64,
    pub device: DeviceRequest,
    pub hardware: Hardware,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct EnvelopeHeader {
    version: i32,
    content_id: Uuid,
    issuer_fingerprint: String,
    hardware_policy: String,
    ephemeral_public_key: String,
    nonce: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct EncryptedEnrollment {
    header: EnvelopeHeader,
    ciphertext: String,
}

impl Issuer {
    pub fn enrollment_secret(&self) -> Result<SecretKey> {
        for counter in 0u8..=255 {
            let mut hash = Sha256::new();
            hash.update(b"OfflineLicenseRequirements/ECDH-key/v2");
            hash.update(self.signing.to_bytes());
            hash.update([counter]);
            let scalar = Zeroizing::new(hash.finalize().to_vec());
            if let Ok(secret) = SecretKey::from_slice(&scalar) {
                return Ok(secret);
            }
        }
        anyhow::bail!(crate::i18n::text("无法生成设备信息加密密钥"))
    }
    pub fn enrollment_public(&self) -> Result<String> {
        Ok(b64(self
            .enrollment_secret()?
            .public_key()
            .to_public_key_der()?
            .as_bytes()))
    }
}
fn encryption_key(
    secret: &SecretKey,
    public: &PublicKey,
    aad: &[u8],
) -> Result<Zeroizing<[u8; 32]>> {
    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    let salt = Sha256::digest(aad);
    let hkdf = hkdf::Hkdf::<Sha256>::new(Some(&salt), shared.raw_secret_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hkdf.expand(
        b"OfflineLicenseRequirements/Enrollment/AES256GCM/v2",
        &mut *key,
    )
    .map_err(|_| anyhow::anyhow!(crate::i18n::text("设备信息密钥派生失败")))?;
    Ok(key)
}
pub fn export(info: &PackageInfo, device: &Device) -> Result<EncryptedEnrollment> {
    ensure!(
        info.version == 2,
        crate::i18n::text("旧内容包不支持新设备信息流程，请重新打包")
    );
    let hardware = device
        .hardware
        .as_ref()
        .context(crate::i18n::text("缺少完整硬件信息"))?
        .clone();
    let payload = Enrollment {
        version: 2,
        request_id: Uuid::new_v4(),
        content_id: info.content_id,
        issuer_fingerprint: info.issuer_fingerprint.clone(),
        hardware_policy: info.hardware_policy.clone(),
        created_at_utc_seconds: now(),
        device: device.request()?,
        hardware,
    };
    let ephemeral = SecretKey::random(&mut rand::rngs::OsRng);
    let header = EnvelopeHeader {
        version: 2,
        content_id: info.content_id,
        issuer_fingerprint: info.issuer_fingerprint.clone(),
        hardware_policy: info.hardware_policy.clone(),
        ephemeral_public_key: b64(ephemeral.public_key().to_public_key_der()?.as_bytes()),
        nonce: b64(&random::<12>()),
    };
    let aad = encode(&header)?;
    let public = PublicKey::from_public_key_der(&unb64(&info.enrollment_public_key)?)?;
    let key = encryption_key(&ephemeral, &public, &aad)?;
    let aes = Aes256Gcm::new_from_slice(&*key)?;
    let nonce = unb64(&header.nonce)?;
    let plain = Zeroizing::new(encode(&payload)?);
    let cipher = aes
        .encrypt(
            nonce.as_slice().into(),
            Payload {
                msg: &plain,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!(crate::i18n::text("设备信息加密失败")))?;
    Ok(EncryptedEnrollment {
        header,
        ciphertext: b64(&cipher),
    })
}
pub fn decrypt(
    issuer: &Issuer,
    info: &PackageInfo,
    envelope: &EncryptedEnrollment,
) -> Result<Enrollment> {
    let h = &envelope.header;
    ensure!(
        h.version == 2
            && info.version == 2
            && h.content_id == info.content_id
            && h.issuer_fingerprint == info.issuer_fingerprint
            && h.hardware_policy == info.hardware_policy,
        crate::i18n::text("设备信息不属于当前内容包")
    );
    ensure!(
        info.enrollment_public_key == issuer.enrollment_public()?,
        crate::i18n::text("发行者设备加密密钥不符")
    );
    let nonce = unb64(&h.nonce)?;
    ensure!(nonce.len() == 12, crate::i18n::text("设备信息随机数无效"));
    let public = PublicKey::from_public_key_der(&unb64(&h.ephemeral_public_key)?)?;
    let aad = encode(h)?;
    let key = encryption_key(&issuer.enrollment_secret()?, &public, &aad)?;
    let aes = Aes256Gcm::new_from_slice(&*key)?;
    let cipher = unb64(&envelope.ciphertext)?;
    ensure!(cipher.len() <= 262144, crate::i18n::text("设备信息过大"));
    let plain = Zeroizing::new(
        aes.decrypt(
            nonce.as_slice().into(),
            Payload {
                msg: &cipher,
                aad: &aad,
            },
        )
        .map_err(|_| {
            anyhow::anyhow!(crate::i18n::text(
                "设备信息无法解密，文件已损坏或发行者密钥不匹配"
            ))
        })?,
    );
    let e: Enrollment = decode(&plain, 262144)?;
    ensure!(
        e.version == 2
            && !e.request_id.is_nil()
            && e.content_id == info.content_id
            && e.issuer_fingerprint == info.issuer_fingerprint
            && e.hardware_policy == info.hardware_policy
            && e.device.version == 2,
        crate::i18n::text("设备信息上下文无效")
    );
    ensure!(
        e.device.device_id == e.hardware.id(&info.hardware_policy)?,
        crate::i18n::text("硬件信息与设备识别码不一致")
    );
    validate_request(&e.device)?;
    Ok(e)
}
pub fn read_enrollment(path: &Path) -> Result<EncryptedEnrollment> {
    ensure!(
        std::fs::metadata(path)?.len() <= 393216,
        crate::i18n::text("设备信息文件过大")
    );
    let b = std::fs::read(path)?;
    decode(&b, 393216)
}
pub fn request_for(issuer: &Issuer, info: &PackageInfo, path: &Path) -> Result<DeviceRequest> {
    if info.version == 2 {
        Ok(decrypt(issuer, info, &read_enrollment(path)?)?.device)
    } else {
        read_json(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(values: &[(&str, &str)]) -> Row {
        values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    pub fn sample() -> Hardware {
        Hardware {
            computer: vec![row(&[("Manufacturer", "Brand"), ("Model", "Model")])],
            base_board: vec![row(&[
                ("Manufacturer", "Brand"),
                ("Product", "Board"),
                ("SerialNumber", "12345"),
            ])],
            computer_product: vec![row(&[
                ("Name", "Model"),
                ("Vendor", "Brand"),
                ("UUID", "00112233-4455-6677-8899-aabbccddeeff"),
            ])],
            disks: vec![row(&[
                ("BusType", "17"),
                ("Model", "SSD"),
                ("SerialNumber", "Disk123"),
                ("InterfaceType", "SCSI"),
                ("PNPDeviceID", "SCSI\\DISK"),
            ])],
            processors: vec![row(&[
                ("Name", "CPU"),
                ("SerialNumber", "Unknown"),
                ("ProcessorId", "ABCD"),
            ])],
            bios: vec![row(&[("SerialNumber", "OEM")])],
            memory: vec![row(&[("DeviceLocator", "A2"), ("SerialNumber", "M123")])],
            network_adapters: vec![row(&[
                ("Name", "Ethernet"),
                ("MACAddress", "11:22:33:44:55:66"),
                ("NetConnectionStatus", "2"),
            ])],
        }
    }
    #[test]
    fn fingerprint_stability_and_hardware_changes() -> Result<()> {
        let a = sample();
        let stable = a.id(STABLE)?;
        let strict = a.id(STRICT)?;
        let mut b = a.clone();
        b.network_adapters[0].insert("NetConnectionStatus".into(), "7".into());
        b.disks.push(row(&[
            ("BusType", "7"),
            ("Model", "USB"),
            ("SerialNumber", "usb123"),
            ("InterfaceType", "USB"),
            ("PNPDeviceID", "USBSTOR\\disk"),
        ]));
        assert_eq!(stable, b.id(STABLE)?);
        b.disks
            .last_mut()
            .unwrap()
            .insert("InterfaceType".into(), "SCSI".into());
        b.disks
            .last_mut()
            .unwrap()
            .insert("PNPDeviceID".into(), "SCSI\\disk".into());
        assert_eq!(stable, b.id(STABLE)?);
        assert_ne!(strict, b.id(STRICT)?);
        b.memory[0].insert("SerialNumber".into(), "changed".into());
        assert_ne!(stable, b.id(STABLE)?);
        let mut c = a.clone();
        c.processors[0].insert("IPAddress".into(), "1.2.3.4".into());
        assert!(c.id(STABLE).is_err());
        Ok(())
    }
    #[test]
    fn encrypted_enrollment_and_lic_v2_roundtrip() -> Result<()> {
        let issuer = Issuer::create()?;
        let public = issuer.spki()?;
        let info = PackageInfo {
            custom_activation_window: true,
            version: 2,
            content_id: Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&public),
            issuer_public_key: b64(&public),
            display_name: "test.txt".into(),
            kind: "text".into(),
            plain_length: 5,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: STABLE.into(),
            enrollment_public_key: issuer.enrollment_public()?,
        };
        let device = Device::with_hardware(sample(), STABLE)?;
        let encrypted = export(&info, &device)?;
        let wire = String::from_utf8(encode(&encrypted)?)?;
        assert!(!wire.contains("Disk123"));
        assert!(!wire.contains("11:22:33:44:55:66"));
        assert!(!wire.contains("ProcessorId"));
        let enrollment = decrypt(&issuer, &info, &encrypted)?;
        assert_eq!(enrollment.device.device_id, device.id);
        assert_eq!(enrollment.hardware.id(STABLE)?, device.id);
        let mut tampered = encrypted.clone();
        let mut bytes = unb64(&tampered.ciphertext)?;
        bytes[0] ^= 1;
        tampered.ciphertext = b64(&bytes);
        assert!(decrypt(&issuer, &info, &tampered).is_err());
        let mut wrong_info = info.clone();
        wrong_info.content_id = Uuid::new_v4();
        assert!(decrypt(&issuer, &wrong_info, &encrypted).is_err());
        assert!(decrypt(&Issuer::create()?, &info, &encrypted).is_err());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("v2-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let input = dir.join("input.txt");
        let pkg = dir.join("encrypted.pkg");
        let exe = dir.join("Content.exe");
        let license_path = dir.join("key.lic");
        std::fs::write(&input, b"hello")?;
        let key = random::<32>();
        write_package(&input, &pkg, &info, &key)?;
        build_exe(&std::env::current_exe()?, &pkg, &exe)?;
        let catalog = Catalog {
            info: info.clone(),
            key_base64: b64(&key),
        };
        let license = issue(&issuer, &catalog, &enrollment.device, 180, now())?;
        atomic(&license_path, &encode(&license)?)?;
        let mut session = crate::session::Session::new(info.clone());
        assert_eq!(session.import(&license_path, &device, &exe)?, key);
        let body = verify(&license, &info, &device.id)?;
        assert_eq!(body.version, 2);
        let mut changed = sample();
        changed.memory[0].insert("SerialNumber".into(), "OTHER".into());
        let changed_device = Device::with_hardware(changed, STABLE)?;
        assert!(verify(&license, &info, &changed_device.id).is_err());
        let mut loaded = crate::session::Session::new(info.clone());
        assert_eq!(loaded.resume(&device)?.unwrap(), key);
        std::fs::remove_file(crate::platform::state_path(info.content_id))?;
        assert!(dir.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join("work")));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
