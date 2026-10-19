use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit,
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey},
};
use rand::{rngs::OsRng, RngCore};
use rsa::{traits::PublicKeyParts, Oaep, RsaPublicKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const CHUNK: usize = 1024 * 1024;
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    OsRng.fill_bytes(&mut b);
    b
}
pub fn b64(b: &[u8]) -> String {
    B64.encode(b)
}
pub fn unb64(s: &str) -> Result<Vec<u8>> {
    Ok(B64.decode(s)?)
}
pub fn fingerprint(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|v| format!("{v:02X}"))
        .collect()
}
pub fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(v)?)
}
pub fn decode<T: DeserializeOwned>(b: &[u8], limit: usize) -> Result<T> {
    ensure!(b.len() <= limit, crate::i18n::text("文件大小超出限制"));
    Ok(serde_json::from_slice(b)?)
}
pub fn read_json<T: DeserializeOwned>(p: &Path) -> Result<T> {
    ensure!(
        fs::metadata(p)?.len() <= 65536,
        crate::i18n::text("文件过大")
    );
    decode(&fs::read(p)?, 65536)
}
pub fn atomic(p: &Path, b: &[u8]) -> Result<()> {
    if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(b)?;
        f.sync_all()?;
        drop(f);
        crate::platform::replace_file(&tmp, p)
    })();
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
pub fn device_id(uuid: &str) -> Result<String> {
    let h = uuid.replace('-', "");
    ensure!(
        h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()),
        crate::i18n::text("硬件 UUID 格式无效")
    );
    let n = u128::from_str_radix(&h, 16)?;
    ensure!(
        n != 0 && n != u128::MAX,
        crate::i18n::text("硬件 UUID 无效")
    );
    let d = md5::Md5::digest(n.to_string().as_bytes());
    Ok(u128::from_be_bytes(d.into()).to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct DeviceRequest {
    pub version: i32,
    pub device_id: String,
    pub rsa_public_key: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct PackageInfo {
    #[serde(default)]
    pub custom_activation_window: bool,
    pub version: i32,
    pub content_id: Uuid,
    pub issuer_fingerprint: String,
    pub issuer_public_key: String,
    pub display_name: String,
    pub kind: String,
    pub plain_length: u64,
    pub chunk_size: usize,
    pub nonce_prefix: String,
    #[serde(default)]
    pub hardware_policy: String,
    #[serde(default)]
    pub enrollment_public_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct LicenseBody {
    pub version: i32,
    pub license_id: Uuid,
    pub content_id: Uuid,
    pub issuer_fingerprint: String,
    pub device_id: String,
    pub issued_at_utc_seconds: i64,
    pub activate_before_utc_seconds: i64,
    pub days: i32,
    pub wrapped_content_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct SignedLicense {
    pub body_base64: String,
    pub signature_base64: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct ActivationRecord {
    pub license: SignedLicense,
    pub activated_at_utc_seconds: i64,
    pub expires_at_utc_seconds: i64,
    pub last_seen_utc_seconds: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct ActivationState {
    pub current: Option<ActivationRecord>,
    pub used: Vec<Uuid>,
    pub locked: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Catalog {
    pub info: PackageInfo,
    pub key_base64: String,
}

pub fn expiry(start: i64, days: i32) -> Result<i64> {
    ensure!(days > 0, crate::i18n::text("授权天数必须为正整数"));
    let end = start
        .checked_add(i64::from(days) * 86400)
        .context(crate::i18n::text("授权日期溢出"))?;
    ensure!(
        (-62135596800..=253402300799).contains(&start) && end <= 253402300799,
        crate::i18n::text("授权日期超出范围")
    );
    Ok(end)
}
pub fn validate_body(b: &LicenseBody) -> Result<()> {
    ensure!(
        (b.version == 1 || b.version == 2) && !b.license_id.is_nil() && !b.content_id.is_nil(),
        crate::i18n::text("授权版本或编号无效")
    );
    ensure!(
        valid_device_id(b.version, &b.device_id),
        crate::i18n::text("设备编号无效")
    );
    ensure!(
        b.issuer_fingerprint.len() == 64
            && b.issuer_fingerprint.bytes().all(|c| c.is_ascii_hexdigit()),
        crate::i18n::text("发行者指纹无效")
    );
    ensure!(
        unb64(&b.wrapped_content_key)?.len() == 384,
        crate::i18n::text("包装密钥无效")
    );
    ensure!(
        b.activate_before_utc_seconds
            .checked_sub(b.issued_at_utc_seconds)
            .is_some_and(|seconds| seconds > 0)
            && b.activate_before_utc_seconds <= 253402300799,
        crate::i18n::text("激活窗口无效")
    );
    expiry(b.issued_at_utc_seconds, b.days)?;
    Ok(())
}
pub fn public(info: &PackageInfo) -> Result<VerifyingKey> {
    let der = unb64(&info.issuer_public_key)?;
    ensure!(
        fingerprint(&der) == info.issuer_fingerprint,
        crate::i18n::text("发行者指纹不符")
    );
    Ok(VerifyingKey::from_public_key_der(&der)?)
}
pub fn verify(s: &SignedLicense, info: &PackageInfo, device: &str) -> Result<LicenseBody> {
    ensure!(
        s.body_base64.len() <= 48000 && s.signature_base64.len() == 88,
        crate::i18n::text("授权格式无效")
    );
    let raw = unb64(&s.body_base64)?;
    let sig = Signature::from_slice(&unb64(&s.signature_base64)?)?;
    public(info)?
        .verify(&raw, &sig)
        .map_err(|_| anyhow::anyhow!(crate::i18n::text("授权签名无效")))?;
    let b: LicenseBody = decode(&raw, 65536)?;
    validate_body(&b)?;
    ensure!(
        b.version == info.version,
        crate::i18n::text("授权协议版本与内容包不一致")
    );
    ensure!(
        b.issuer_fingerprint == info.issuer_fingerprint && b.content_id == info.content_id,
        crate::i18n::text("授权不属于该内容")
    );
    ensure!(b.device_id == device, crate::i18n::text("授权不属于该设备"));
    Ok(b)
}
pub fn activate(s: SignedLicense, b: &LicenseBody, time: i64) -> Result<ActivationRecord> {
    ensure!(
        time >= b.issued_at_utc_seconds,
        crate::i18n::text("授权签发时间在未来")
    );
    ensure!(
        time < b.activate_before_utc_seconds,
        crate::i18n::text("授权已超过首次导入窗口，请管理员重新签发")
    );
    Ok(ActivationRecord {
        license: s,
        activated_at_utc_seconds: time,
        expires_at_utc_seconds: expiry(time, b.days)?,
        last_seen_utc_seconds: time,
    })
}
pub fn check(r: &ActivationRecord, b: &LicenseBody, time: i64) -> Result<()> {
    ensure!(
        r.activated_at_utc_seconds >= b.issued_at_utc_seconds
            && r.activated_at_utc_seconds < b.activate_before_utc_seconds
            && r.expires_at_utc_seconds == expiry(r.activated_at_utc_seconds, b.days)?
            && r.last_seen_utc_seconds >= r.activated_at_utc_seconds,
        crate::i18n::text("本地授权状态损坏")
    );
    ensure!(
        time >= r.last_seen_utc_seconds - 2,
        crate::i18n::text("检测到时间回拨，请校准时间并申请新授权")
    );
    ensure!(
        time < r.expires_at_utc_seconds,
        crate::i18n::text("使用期限已结束，请申请新授权")
    );
    Ok(())
}

pub struct Issuer {
    pub raw: Zeroizing<Vec<u8>>,
    pub signing: SigningKey,
}
impl Issuer {
    pub fn create() -> Result<Self> {
        let k = SigningKey::random(&mut OsRng);
        let raw = canonical_key(&k);
        Ok(Self { raw, signing: k })
    }
    pub fn load(path: &Path, password: &str) -> Result<Self> {
        ensure!(
            fs::metadata(path)?.len() <= 16384,
            crate::i18n::text("密钥文件过大")
        );
        let bytes = fs::read(path)?;
        let e = pkcs8::EncryptedPrivateKeyInfo::try_from(bytes.as_slice())?;
        let doc = e
            .decrypt(password)
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("密钥口令错误或文件损坏")))?;
        let raw = Zeroizing::new(doc.as_bytes().to_vec());
        let signing = SigningKey::from_pkcs8_der(&raw)?;
        let raw = canonical_key(&signing);
        Ok(Self { raw, signing })
    }
    pub fn save(&self, path: &Path, password: &str) -> Result<()> {
        use pkcs8::der::Encode;
        ensure!(
            password.chars().count() >= 8,
            crate::i18n::text("口令至少 8 个字符")
        );
        let salt = random::<16>();
        let iv = random::<16>();
        let params = pkcs5::pbes2::Parameters::pbkdf2_sha256_aes256cbc(600000, &salt, &iv)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let cipher = params
            .encrypt(password, &self.raw)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let doc = pkcs8::EncryptedPrivateKeyInfo {
            encryption_algorithm: pkcs5::EncryptionScheme::Pbes2(params),
            encrypted_data: &cipher,
        }
        .to_der()?;
        atomic(path, &doc)
    }
    pub fn spki(&self) -> Result<Vec<u8>> {
        Ok(self
            .signing
            .verifying_key()
            .to_public_key_der()?
            .as_bytes()
            .to_vec())
    }
    pub fn sign(&self, b: &LicenseBody) -> Result<SignedLicense> {
        validate_body(b)?;
        let raw = encode(b)?;
        let sig: Signature = self.signing.sign(&raw);
        Ok(SignedLicense {
            body_base64: b64(&raw),
            signature_base64: b64(&sig.to_bytes()),
        })
    }
    pub fn catalog_save(&self, p: &Path, c: &Catalog) -> Result<()> {
        let nonce = random::<12>();
        let key = Sha256::digest(&self.raw);
        let aes = Aes256Gcm::new_from_slice(&key)?;
        let plain = Zeroizing::new(encode(c)?);
        let encrypted = aes
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: &plain,
                    aad: b"OLMETA01",
                },
            )
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("包记录加密失败")))?;
        let (cipher, tag) = encrypted.split_at(encrypted.len() - 16);
        let mut out = b"OLMETA01".to_vec();
        out.extend_from_slice(&nonce);
        out.extend_from_slice(tag);
        out.extend_from_slice(cipher);
        atomic(p, &out)
    }
    pub fn catalog_load(&self, p: &Path) -> Result<Catalog> {
        ensure!(
            fs::metadata(p)?.len() <= 65536,
            crate::i18n::text("包记录过大")
        );
        let b = fs::read(p)?;
        ensure!(
            b.len() >= 36 && &b[..8] == b"OLMETA01",
            crate::i18n::text("管理员记录无效")
        );
        let mut cipher = b[36..].to_vec();
        cipher.extend_from_slice(&b[20..36]);
        let key = Sha256::digest(&self.raw);
        let aes = Aes256Gcm::new_from_slice(&key)?;
        let plain = Zeroizing::new(
            aes.decrypt(
                (&b[8..20]).into(),
                Payload {
                    msg: &cipher,
                    aad: b"OLMETA01",
                },
            )
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("记录不属于当前发行者或已损坏")))?,
        );
        let c: Catalog = decode(&plain, 65536)?;
        ensure!(
            c.info.issuer_fingerprint == fingerprint(&self.spki()?)
                && unb64(&c.key_base64)?.len() == 32,
            crate::i18n::text("包记录无效")
        );
        Ok(c)
    }
}
fn valid_device_id(version: i32, id: &str) -> bool {
    match version {
        1 => (1..=39).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_digit()),
        2 => id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()),
        _ => false,
    }
}
pub fn validate_request(r: &DeviceRequest) -> Result<RsaPublicKey> {
    ensure!(
        valid_device_id(r.version, &r.device_id),
        crate::i18n::text("设备请求无效")
    );
    let k = RsaPublicKey::from_public_key_der(&unb64(&r.rsa_public_key)?)?;
    ensure!(
        k.n().bits() == 3072,
        crate::i18n::text("设备 RSA 公钥须为 3072 位")
    );
    Ok(k)
}
pub fn issue(
    issuer: &Issuer,
    c: &Catalog,
    r: &DeviceRequest,
    days: i32,
    time: i64,
) -> Result<SignedLicense> {
    issue_with_window(issuer, c, r, days, 60, time)
}
pub fn activation_deadline(time: i64, seconds: i64) -> Result<i64> {
    ensure!(seconds > 0, crate::i18n::text("首次导入窗口必须大于零"));
    let end = time
        .checked_add(seconds)
        .context(crate::i18n::text("首次导入窗口数值过大"))?;
    ensure!(
        (-62135596800..=253402300799).contains(&time) && end <= 253402300799,
        crate::i18n::text("首次导入窗口日期超出范围")
    );
    Ok(end)
}
pub fn issue_with_window(
    issuer: &Issuer,
    c: &Catalog,
    r: &DeviceRequest,
    days: i32,
    window_seconds: i64,
    time: i64,
) -> Result<SignedLicense> {
    expiry(time, days)?;
    let deadline = activation_deadline(time, window_seconds)?;
    expiry(deadline - 1, days)?;
    ensure!(
        c.info.custom_activation_window || window_seconds == 60,
        crate::i18n::text("此内容由旧版打包，仅支持60秒。自定义窗口请重新打包原文件。")
    );
    ensure!(
        r.version == c.info.version,
        crate::i18n::text("设备请求与内容包版本不一致")
    );
    let rsa = validate_request(r)?;
    let key = Zeroizing::new(unb64(&c.key_base64)?);
    let wrapped = rsa.encrypt(&mut OsRng, Oaep::new::<Sha256>(), &key)?;
    issuer.sign(&LicenseBody {
        version: c.info.version,
        license_id: Uuid::new_v4(),
        content_id: c.info.content_id,
        issuer_fingerprint: c.info.issuer_fingerprint.clone(),
        device_id: r.device_id.clone(),
        issued_at_utc_seconds: time,
        activate_before_utc_seconds: deadline,
        days,
        wrapped_content_key: b64(&wrapped),
    })
}

pub struct Package {
    pub file: File,
    pub base: u64,
    pub length: u64,
    pub info: PackageInfo,
    pub data_start: u64,
    pub header_hash: [u8; 32],
}
impl Package {
    pub fn open_exe(path: &Path) -> Result<Self> {
        let mut f = File::open(path)?;
        let n = f.metadata()?.len();
        ensure!(n >= 32, crate::i18n::text("此程序没有内容包"));
        f.seek(SeekFrom::End(-32))?;
        let mut foot = [0; 32];
        f.read_exact(&mut foot)?;
        ensure!(
            &foot[..8] == b"OLEXE001" && u64::from_le_bytes(foot[24..32].try_into()?) == 1,
            crate::i18n::text("内容 EXE 格式无效")
        );
        let base = u64::from_le_bytes(foot[8..16].try_into()?);
        let length = u64::from_le_bytes(foot[16..24].try_into()?);
        ensure!(
            base > 0 && base <= n - 32 && length == n - 32 - base,
            crate::i18n::text("内容包偏移无效")
        );
        Self::header(f, base, length)
    }
    pub fn header(mut file: File, base: u64, length: u64) -> Result<Self> {
        file.seek(SeekFrom::Start(base))?;
        let mut pre = [0; 12];
        file.read_exact(&mut pre)?;
        ensure!(&pre[..8] == b"OLPK0001", crate::i18n::text("内容包头无效"));
        let n = u32::from_le_bytes(pre[8..12].try_into()?) as usize;
        ensure!((1..=65536).contains(&n), crate::i18n::text("包头长度无效"));
        let mut raw = vec![0; n];
        file.read_exact(&mut raw)?;
        let info: PackageInfo = decode(&raw, 65536)?;
        ensure!(
            (info.version == 1 || info.version == 2)
                && !info.content_id.is_nil()
                && info.plain_length > 0
                && info.plain_length <= 4 * 1024_u64.pow(4)
                && info.chunk_size == CHUNK
                && unb64(&info.nonce_prefix)?.len() == 8
                && info.display_name.chars().count() <= 255,
            crate::i18n::text("内容包参数无效")
        );
        ensure!(
            ["media", "pdf", "text", "image", "presentation"].contains(&info.kind.as_str()),
            crate::i18n::text("不支持的查看格式")
        );
        public(&info)?;
        if info.version == 2 {
            ensure!(
                info.hardware_policy == crate::hardware::STABLE
                    || info.hardware_policy == crate::hardware::STRICT,
                crate::i18n::text("硬件绑定策略无效")
            );
            p256::PublicKey::from_public_key_der(&unb64(&info.enrollment_public_key)?)?;
        }
        let count = info.plain_length.div_ceil(CHUNK as u64);
        ensure!(
            length == 12 + n as u64 + info.plain_length + count * 16,
            crate::i18n::text("内容包截断或长度不符")
        );
        Ok(Self {
            file,
            base,
            length,
            info,
            data_start: base + 12 + n as u64,
            header_hash: Sha256::digest(&raw).into(),
        })
    }
    pub fn encrypted_hash(&mut self) -> Result<[u8; 32]> {
        self.file.seek(SeekFrom::Start(self.base))?;
        let mut h = Sha256::new();
        let mut remaining = self.length;
        let mut buf = vec![0; CHUNK];
        while remaining > 0 {
            let n = remaining.min(CHUNK as u64) as usize;
            self.file.read_exact(&mut buf[..n])?;
            h.update(&buf[..n]);
            remaining -= n as u64;
        }
        Ok(h.finalize().into())
    }
}
pub struct Reader {
    pub package: Package,
    pub key: Zeroizing<Vec<u8>>,
    pub position: u64,
    pub cache: Zeroizing<Vec<u8>>,
    pub cached: Option<u64>,
}
impl Reader {
    pub fn new(package: Package, key: Vec<u8>) -> Result<Self> {
        ensure!(key.len() == 32, crate::i18n::text("内容密钥无效"));
        Ok(Self {
            package,
            key: Zeroizing::new(key),
            position: 0,
            cache: Zeroizing::new(vec![]),
            cached: None,
        })
    }
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let mut total = 0;
        while total < out.len() && self.position < self.package.info.plain_length {
            let index = self.position / CHUNK as u64;
            if self.cached != Some(index) {
                self.cache.zeroize();
                let size = (self.package.info.plain_length - index * CHUNK as u64).min(CHUNK as u64)
                    as usize;
                self.package.file.seek(SeekFrom::Start(
                    self.package.data_start + index * (CHUNK as u64 + 16),
                ))?;
                let mut b = vec![0; size + 16];
                self.package.file.read_exact(&mut b)?;
                let mut nonce = unb64(&self.package.info.nonce_prefix)?;
                nonce.extend_from_slice(&(index as u32).to_be_bytes());
                let mut aad = self.package.header_hash.to_vec();
                aad.extend_from_slice(&(index as u32).to_be_bytes());
                let aes = Aes256Gcm::new_from_slice(&self.key)?;
                self.cache = Zeroizing::new(
                    aes.decrypt(nonce.as_slice().into(), Payload { msg: &b, aad: &aad })
                        .map_err(|_| anyhow::anyhow!(crate::i18n::text("内容密文校验失败")))?,
                );
                self.cached = Some(index);
            }
            let offset = (self.position % CHUNK as u64) as usize;
            let n = (out.len() - total).min(self.cache.len() - offset);
            out[total..total + n].copy_from_slice(&self.cache[offset..offset + n]);
            total += n;
            self.position += n as u64;
        }
        Ok(total)
    }
    pub fn seek(&mut self, p: u64) -> Result<()> {
        ensure!(
            p <= self.package.info.plain_length,
            crate::i18n::text("无效定位")
        );
        self.position = p;
        Ok(())
    }
    pub fn all(&mut self, limit: u64) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(
            self.package.info.plain_length <= limit,
            crate::i18n::text("内容超过查看大小限制")
        );
        self.seek(0)?;
        let mut b = Zeroizing::new(vec![0; self.package.info.plain_length as usize]);
        let n = self.read(&mut b)?;
        ensure!(n == b.len(), crate::i18n::text("内容读取不完整"));
        Ok(b)
    }
}
pub fn write_package(input: &Path, out: &Path, info: &PackageInfo, key: &[u8]) -> Result<()> {
    let mut f = File::open(input)?;
    let mut o = File::create(out)?;
    let raw = encode(info)?;
    o.write_all(b"OLPK0001")?;
    o.write_all(&(raw.len() as u32).to_le_bytes())?;
    o.write_all(&raw)?;
    let hash = Sha256::digest(&raw);
    let prefix = unb64(&info.nonce_prefix)?;
    let aes = Aes256Gcm::new_from_slice(key)?;
    let mut done = 0;
    let mut index = 0u32;
    let mut buf = Zeroizing::new(vec![0; CHUNK]);
    while done < info.plain_length {
        let n = (info.plain_length - done).min(CHUNK as u64) as usize;
        f.read_exact(&mut buf[..n])?;
        let mut nonce = prefix.clone();
        nonce.extend_from_slice(&index.to_be_bytes());
        let mut aad = hash.to_vec();
        aad.extend_from_slice(&index.to_be_bytes());
        let b = aes
            .encrypt(
                nonce.as_slice().into(),
                Payload {
                    msg: &buf[..n],
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("内容加密失败")))?;
        o.write_all(&b)?;
        done += n as u64;
        index += 1;
    }
    let mut extra = [0];
    ensure!(
        f.read(&mut extra)? == 0,
        crate::i18n::text("输入文件已变化")
    );
    o.sync_all()?;
    Ok(())
}
pub fn build_exe(template: &Path, package: &Path, out: &Path) -> Result<()> {
    ensure!(template != out, crate::i18n::text("不能覆盖管理程序"));
    let temp = out.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut o = File::options()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&temp)?;
        std::io::copy(&mut File::open(template)?, &mut o)?;
        let offset = o.stream_position()?;
        let len = std::io::copy(&mut File::open(package)?, &mut o)?;
        o.write_all(b"OLEXE001")?;
        o.write_all(&offset.to_le_bytes())?;
        o.write_all(&len.to_le_bytes())?;
        o.write_all(&1u64.to_le_bytes())?;
        o.sync_all()?;
        o.set_permissions(fs::Permissions::from_mode(0o700))?;
        drop(o);
        crate::platform::replace_file(&temp, out)
    })();
    if temp.exists() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_chunks_signatures_and_replay() -> Result<()> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let issuer = Issuer::create()?;
        let spki = issuer.spki()?;
        let info = PackageInfo {
            custom_activation_window: true,
            version: 1,
            content_id: Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&spki),
            issuer_public_key: b64(&spki),
            display_name: "test.txt".into(),
            kind: "text".into(),
            plain_length: (CHUNK * 2 + 17) as u64,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: String::new(),
            enrollment_public_key: String::new(),
        };
        let key = random::<32>();
        let plain: Vec<u8> = (0..info.plain_length).map(|n| (n % 251) as u8).collect();
        let input = dir.join("input.txt");
        let pkg = dir.join("package.pkg");
        let exe = dir.join("Content.exe");
        fs::write(&input, &plain)?;
        write_package(&input, &pkg, &info, &key)?;
        build_exe(&std::env::current_exe()?, &pkg, &exe)?;
        let mut reader = Reader::new(Package::open_exe(&exe)?, key.to_vec())?;
        reader.seek((CHUNK - 9) as u64)?;
        let mut probe = [0u8; 40];
        assert_eq!(reader.read(&mut probe)?, 40);
        assert_eq!(&probe, &plain[CHUNK - 9..CHUNK + 31]);
        reader.seek((CHUNK * 2) as u64)?;
        let mut last = [0u8; 20];
        assert_eq!(reader.read(&mut last)?, 17);
        assert_eq!(&last[..17], &plain[CHUNK * 2..]);
        let mut wrong = Reader::new(Package::open_exe(&exe)?, random::<32>().to_vec())?;
        assert!(wrong.read(&mut probe).is_err());
        let mut damaged = fs::read(&pkg)?;
        let n = damaged.len();
        damaged[n - 1] ^= 1;
        let damaged_path = dir.join("damaged.pkg");
        fs::write(&damaged_path, &damaged)?;
        let mut damaged_reader = Reader::new(
            Package::header(File::open(&damaged_path)?, 0, damaged.len() as u64)?,
            key.to_vec(),
        )?;
        damaged_reader.seek((CHUNK * 2) as u64)?;
        assert!(damaged_reader.read(&mut probe).is_err());
        let catalog = Catalog {
            info: info.clone(),
            key_base64: b64(&key),
        };
        let backup = dir.join("backup.issuerkey");
        issuer.save(&backup, "test-only-password")?;
        assert!(Issuer::load(&backup, "wrong-password").is_err());
        let restored = Issuer::load(&backup, "test-only-password")?;
        assert_eq!(restored.spki()?, spki);
        assert_eq!(restored.enrollment_public()?, issuer.enrollment_public()?);
        let meta = dir.join("content.olmeta");
        issuer.catalog_save(&meta, &catalog)?;
        assert_eq!(restored.catalog_load(&meta)?.key_base64, catalog.key_base64);
        let device = crate::platform::Device::load()?;
        let lic = issue(&issuer, &catalog, &device.request()?, 180, now())?;
        let body = verify(&lic, &info, &device.id)?;
        assert_eq!(device.unwrap(&body.wrapped_content_key)?.as_slice(), key);
        for window in [1, 600, 86400] {
            let issued = now();
            let custom =
                issue_with_window(&issuer, &catalog, &device.request()?, 180, window, issued)?;
            let custom_body = verify(&custom, &info, &device.id)?;
            assert_eq!(custom_body.activate_before_utc_seconds - issued, window);
            assert!(activate(custom.clone(), &custom_body, issued + window - 1).is_ok());
            assert!(activate(custom.clone(), &custom_body, issued + window).is_err());
            let mut changed_body = custom_body;
            changed_body.activate_before_utc_seconds += 1;
            let mut forged = custom;
            forged.body_base64 = b64(&encode(&changed_body)?);
            assert!(verify(&forged, &info, &device.id).is_err());
        }
        let mut legacy = catalog.clone();
        legacy.info.custom_activation_window = false;
        assert!(issue_with_window(&issuer, &legacy, &device.request()?, 180, 600, now()).is_err());
        assert!(issue_with_window(&issuer, &legacy, &device.request()?, 180, 60, now()).is_ok());
        let mut legacy_json = serde_json::to_value(&info)?;
        legacy_json
            .as_object_mut()
            .unwrap()
            .remove("CustomActivationWindow");
        assert!(!serde_json::from_value::<PackageInfo>(legacy_json)?.custom_activation_window);
        assert!(verify(&lic, &info, "wrong-device").is_err());
        let mut changed = lic.clone();
        let mut signature = unb64(&changed.signature_base64)?;
        signature[0] ^= 1;
        changed.signature_base64 = b64(&signature);
        assert!(verify(&changed, &info, &device.id).is_err());
        let license_path = dir.join("license.lic");
        atomic(&license_path, &encode(&lic)?)?;
        let mut session = crate::session::Session::new(info.clone());
        session.import(&license_path, &device, &exe)?;
        let start = session
            .state
            .current
            .as_ref()
            .unwrap()
            .activated_at_utc_seconds;
        session.import(&license_path, &device, &exe)?;
        assert_eq!(
            session
                .state
                .current
                .as_ref()
                .unwrap()
                .activated_at_utc_seconds,
            start
        );
        let mut resumed = crate::session::Session::new(info.clone());
        assert_eq!(resumed.resume(&device)?.unwrap(), key);
        resumed.baseline = now() + 10;
        assert!(resumed.validate().is_err());
        resumed.lock();
        assert!(resumed.import(&license_path, &device, &exe).is_err());
        let record = activate(lic.clone(), &body, body.issued_at_utc_seconds)?;
        assert!(check(&record, &body, record.expires_at_utc_seconds).is_err());
        assert!(check(&record, &body, record.last_seen_utc_seconds - 3).is_err());
        fs::remove_file(crate::platform::state_path(info.content_id))?;
        // Only this test's freshly generated directory is removed.
        assert!(dir.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join("work")));
        fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn uuid_vectors() {
        assert!(device_id("00000000-0000-0000-0000-000000000000").is_err());
        assert!(device_id("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF").is_err());
        assert!(device_id("invalid").is_err());
        assert_eq!(
            device_id("00112233-4455-6677-8899-aabbccddeeff").unwrap(),
            "35820973316352794534836043369028201291"
        );
    }
    #[test]
    fn days_boundaries() {
        for d in [1, 180, 365] {
            assert_eq!(
                expiry(1800000000, d).unwrap(),
                1800000000 + i64::from(d) * 86400
            );
        }
        for d in [0, -1, i32::MAX] {
            assert!(expiry(1800000000, d).is_err());
        }
    }
    #[test]
    fn windows_boundaries() {
        let b = LicenseBody {
            version: 1,
            license_id: Uuid::new_v4(),
            content_id: Uuid::new_v4(),
            issuer_fingerprint: "A".repeat(64),
            device_id: "123".into(),
            issued_at_utc_seconds: 1800000000,
            activate_before_utc_seconds: 1800000060,
            days: 180,
            wrapped_content_key: b64(&[0; 384]),
        };
        let s = SignedLicense {
            body_base64: "".into(),
            signature_base64: "".into(),
        };
        for (offset, ok) in [(-1, false), (0, true), (59, true), (60, false), (61, false)] {
            assert_eq!(
                activate(s.clone(), &b, b.issued_at_utc_seconds + offset).is_ok(),
                ok
            );
        }
        for seconds in [1, 60, 90, 3600, 86400, 30 * 86400] {
            let mut custom = b.clone();
            custom.activate_before_utc_seconds =
                activation_deadline(custom.issued_at_utc_seconds, seconds).unwrap();
            assert!(validate_body(&custom).is_ok());
            assert!(activate(
                s.clone(),
                &custom,
                custom.issued_at_utc_seconds + seconds - 1
            )
            .is_ok());
            assert!(activate(s.clone(), &custom, custom.issued_at_utc_seconds + seconds).is_err());
            let record = activate(
                s.clone(),
                &custom,
                custom.issued_at_utc_seconds + seconds - 1,
            )
            .unwrap();
            assert_eq!(
                record.expires_at_utc_seconds,
                record.activated_at_utc_seconds + 180 * 86400
            );
        }
        for seconds in [0, -1, i64::MAX] {
            assert!(activation_deadline(b.issued_at_utc_seconds, seconds).is_err());
        }
        assert!(activation_deadline(253402300799, 1).is_err());
    }
}

fn canonical_key(k: &SigningKey) -> Zeroizing<Vec<u8>> {
    let mut b = vec![
        0x30, 0x81, 0x87, 0x02, 0x01, 0x00, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d,
        0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x04, 0x6d, 0x30,
        0x6b, 0x02, 0x01, 0x01, 0x04, 0x20,
    ];
    b.extend_from_slice(&k.to_bytes());
    b.extend_from_slice(&[0xa1, 0x44, 0x03, 0x42, 0x00]);
    b.extend_from_slice(k.verifying_key().to_encoded_point(false).as_bytes());
    Zeroizing::new(b)
}
