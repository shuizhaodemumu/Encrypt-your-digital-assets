use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use rand::{rngs::OsRng, RngCore};
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroize;

pub const CHUNK: usize = 1024 * 1024;
const MAGIC: &[u8; 8] = b"EAVIDEO1";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn encode(v: &[u8]) -> String {
    B64.encode(v)
}
pub fn decode(s: &str) -> Result<Vec<u8>> {
    Ok(B64.decode(s)?)
}
fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    OsRng.fill_bytes(&mut b);
    b
}
pub fn sha(v: &[u8]) -> String {
    hex::encode(Sha256::digest(v))
}
fn seal(key: &[u8], raw: &[u8], aad: &[u8]) -> Result<Value> {
    let nonce = random::<12>();
    let bytes = Aes256Gcm::new_from_slice(key)
        .map_err(|_| anyhow::anyhow!("密钥长度错误"))?
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: raw, aad })
        .map_err(|_| anyhow::anyhow!("加密失败"))?;
    Ok(json!({"nonce":encode(&nonce),"data":encode(&bytes)}))
}
fn unseal(key: &[u8], v: &Value, aad: &[u8]) -> Result<Vec<u8>> {
    let n = decode(field(v, "nonce")?)?;
    ensure!(n.len() == 12, "nonce 长度错误");
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| anyhow::anyhow!("密钥长度错误"))?
        .decrypt(
            Nonce::from_slice(&n),
            Payload {
                msg: &decode(field(v, "data")?)?,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("口令错误或数据被篡改"))
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().with_context(|| format!("缺少 {k}"))
}
fn number(v: &Value, k: &str) -> Result<u64> {
    v[k].as_u64().with_context(|| format!("缺少 {k}"))
}

#[derive(Serialize, Deserialize)]
pub struct Issuer {
    signing: String,
    request: String,
    master: String,
}
impl Issuer {
    pub fn create() -> Result<Self> {
        let s = SigningKey::random(&mut OsRng);
        let r = RsaPrivateKey::new(&mut OsRng, 3072)?;
        Ok(Self {
            signing: encode(s.to_pkcs8_der()?.as_bytes()),
            request: encode(r.to_pkcs8_der()?.as_bytes()),
            master: encode(&random::<32>()),
        })
    }
    pub fn save(&self, p: &Path, password: &str) -> Result<()> {
        ensure!(password.len() >= 8, "发行密钥口令至少 8 个字符");
        let salt = random::<16>();
        let mut k = [0; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 300_000, &mut k);
        let result = seal(&k, &serde_json::to_vec(self)?, b"android-issuer-v1")?;
        k.zeroize();
        std::fs::write(
            p,
            serde_json::to_vec_pretty(
                &json!({"version":1,"salt":encode(&salt),"encrypted":result}),
            )?,
        )?;
        Ok(())
    }
    pub fn load(p: &Path, password: &str) -> Result<Self> {
        let v: Value = serde_json::from_slice(&std::fs::read(p)?)?;
        let mut k = [0; 32];
        let salt = decode(field(&v, "salt")?)?;
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 300_000, &mut k);
        let decrypted = unseal(&k, &v["encrypted"], b"android-issuer-v1");
        k.zeroize();
        let mut raw = decrypted?;
        let out = serde_json::from_slice(&raw);
        raw.zeroize();
        Ok(out?)
    }
    fn signing_key(&self) -> Result<SigningKey> {
        Ok(SigningKey::from_pkcs8_der(&decode(&self.signing)?)?)
    }
    fn request_key(&self) -> Result<RsaPrivateKey> {
        Ok(RsaPrivateKey::from_pkcs8_der(&decode(&self.request)?)?)
    }
}
fn signed(issuer: &Issuer, body: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(body)?;
    let sig: Signature = issuer.signing_key()?.sign(&bytes);
    Ok(json!({"body":encode(&bytes),"signature":encode(&sig.to_bytes())}))
}
fn verified(v: &Value, public: &str) -> Result<Value> {
    let bytes = decode(field(v, "body")?)?;
    let sig = Signature::from_slice(&decode(field(v, "signature")?)?)?;
    VerifyingKey::from_public_key_der(&decode(public)?)?
        .verify(&bytes, &sig)
        .context("签名无效")?;
    Ok(serde_json::from_slice(&bytes)?)
}
#[derive(Clone)]
pub struct Header {
    pub body: Value,
    pub offset: u64,
    pub aad: Vec<u8>,
}
pub fn inspect(path: &Path) -> Result<Header> {
    let mut f = File::open(path)?;
    let mut magic = [0; 8];
    f.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "不是安卓加密视频包");
    let mut n = [0; 4];
    f.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n) as usize;
    ensure!(n > 0 && n <= 128 * 1024, "视频包头损坏");
    let mut raw = vec![0; n];
    f.read_exact(&mut raw)?;
    let envelope: Value = serde_json::from_slice(&raw)?;
    let untrusted: Value = serde_json::from_slice(&decode(field(&envelope, "body")?)?)?;
    let body = verified(&envelope, field(&untrusted, "signer")?)?;
    ensure!(
        body["version"] == 1 && body["chunk_size"] == CHUNK,
        "不支持的视频包版本"
    );
    let id = field(&body, "id")?;
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "内容标识无效"
    );
    let len = number(&body, "size")?;
    ensure!(len > 0 && len <= 1024 * 1024 * 1024 * 1024, "文件大小无效");
    ensure!(
        decode(field(&body, "nonce_prefix")?)?.len() == 4,
        "nonce 错误"
    );
    let offset = 12 + n as u64;
    ensure!(
        f.metadata()?.len() == offset + len + len.div_ceil(CHUNK as u64) * 16,
        "视频包截断或长度错误"
    );
    Ok(Header {
        body,
        offset,
        aad: Sha256::digest(&raw).to_vec(),
    })
}
pub fn pack(issuer: &Issuer, input: &Path, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "输出文件已存在");
    let mut src = File::open(input)?;
    let size = src.metadata()?.len();
    ensure!(size > 0, "视频为空");
    let key = random::<32>();
    let prefix = random::<4>();
    let body = json!({"version":1,"id":hex::encode(random::<16>()),"name":input.file_name().unwrap().to_string_lossy(),"size":size,"chunk_size":CHUNK,"nonce_prefix":encode(&prefix),"signer":encode(issuer.signing_key()?.verifying_key().to_public_key_der()?.as_bytes()),"request_public":encode(issuer.request_key()?.to_public_key().to_public_key_der()?.as_bytes())});
    let raw = serde_json::to_vec(&signed(issuer, &body)?)?;
    let aad = Sha256::digest(&raw);
    let aes = Aes256Gcm::new_from_slice(&key).unwrap();
    let mut out = File::create(output)?;
    out.write_all(MAGIC)?;
    out.write_all(&(raw.len() as u32).to_le_bytes())?;
    out.write_all(&raw)?;
    let mut buf = vec![0; CHUNK];
    let mut remaining = size;
    let mut index = 0u64;
    while remaining > 0 {
        let n = (remaining.min(CHUNK as u64)) as usize;
        src.read_exact(&mut buf[..n])?;
        let mut nonce = [0; 12];
        nonce[..4].copy_from_slice(&prefix);
        nonce[4..].copy_from_slice(&index.to_be_bytes());
        let encrypted = aes
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &buf[..n],
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("加密失败"))?;
        out.write_all(&encrypted)?;
        remaining -= n as u64;
        index += 1;
    }
    buf.zeroize();
    out.sync_all()?;
    let meta = seal(
        &decode(&issuer.master)?,
        &key,
        field(&body, "id")?.as_bytes(),
    )?;
    std::fs::write(
        output.with_extension("eav.adminmeta"),
        serde_json::to_vec_pretty(&meta)?,
    )?;
    Ok(())
}
pub fn enrollment(header: &Header, device: &Value) -> Result<Value> {
    let pubbytes = decode(field(device, "public_key")?)?;
    let public = RsaPublicKey::from_public_key_der(&pubbytes)?;
    use rsa::traits::PublicKeyParts;
    ensure!(public.n().bits() >= 2048, "设备密钥过短");
    let payload = json!({"version":1,"device_id":sha(&pubbytes),"public_key":field(device,"public_key")?,"content_id":field(&header.body,"id")?,"model":device["model"],"manufacturer":device["manufacturer"],"android":device["android"]});
    let k = random::<32>();
    let issuer =
        RsaPublicKey::from_public_key_der(&decode(field(&header.body, "request_public")?)?)?;
    Ok(
        json!({"version":1,"key":encode(&issuer.encrypt(&mut OsRng,Oaep::new::<Sha256>(),&k)?),"encrypted":seal(&k,&serde_json::to_vec(&payload)?,b"android-enrollment-v1")?}),
    )
}
pub fn issue(
    issuer: &Issuer,
    package: &Path,
    request: &Value,
    days: u64,
    window: u64,
) -> Result<Value> {
    ensure!(
        (1..=36500).contains(&days) && (1..=31536000).contains(&window),
        "授权期限无效"
    );
    let h = inspect(package)?;
    ensure!(
        encode(
            issuer
                .signing_key()?
                .verifying_key()
                .to_public_key_der()?
                .as_bytes()
        ) == field(&h.body, "signer")?,
        "发行密钥不匹配"
    );
    let mut key = issuer
        .request_key()?
        .decrypt(Oaep::new::<Sha256>(), &decode(field(request, "key")?)?)?;
    let mut raw = unseal(&key, &request["encrypted"], b"android-enrollment-v1")?;
    key.zeroize();
    let r: Value = serde_json::from_slice(&raw)?;
    raw.zeroize();
    ensure!(r["content_id"] == h.body["id"], "设备请求不属于此视频");
    let pubbytes = decode(field(&r, "public_key")?)?;
    ensure!(sha(&pubbytes) == field(&r, "device_id")?, "设备标识不符");
    let meta: Value =
        serde_json::from_slice(&std::fs::read(package.with_extension("eav.adminmeta"))?)?;
    let mut contentkey = unseal(
        &decode(&issuer.master)?,
        &meta,
        field(&h.body, "id")?.as_bytes(),
    )?;
    // AndroidKeyStore OAEP: SHA-256 message digest, explicitly selected SHA-1 MGF1.
    let wrapped = RsaPublicKey::from_public_key_der(&pubbytes)?.encrypt(
        &mut OsRng,
        Oaep::new_with_mgf_hash::<Sha256, sha1::Sha1>(),
        &contentkey,
    )?;
    contentkey.zeroize();
    let issued = now();
    signed(
        issuer,
        &json!({"version":1,"license_id":hex::encode(random::<16>()),"content_id":h.body["id"],"device_id":r["device_id"],"issued":issued,"import_deadline":issued+window,"duration":days*86400,"wrapped_key":encode(&wrapped),"key_scheme":"RSA-OAEP-SHA256-MGF1-SHA1"}),
    )
}
pub fn verify_license(h: &Header, lic: &Value, device: &str) -> Result<Value> {
    let b = verified(lic, field(&h.body, "signer")?)?;
    ensure!(b["version"] == 1, "授权版本不支持");
    ensure!(b["content_id"] == h.body["id"], "授权不属于此视频");
    ensure!(field(&b, "device_id")? == device, "授权不属于此设备");
    ensure!(
        (1..=36500 * 86400).contains(&number(&b, "duration")?),
        "期限不正确"
    );
    ensure!(
        number(&b, "issued")? <= number(&b, "import_deadline")?,
        "授权时间无效"
    );
    Ok(b)
}
pub struct Reader {
    file: File,
    header: Header,
    key: Vec<u8>,
    chunk: u64,
    cache: Vec<u8>,
    expires: u64,
    deadline: Instant,
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.key.zeroize();
        self.cache.zeroize();
    }
}
impl Reader {
    pub fn new(path: &Path, key: Vec<u8>, expires: u64) -> Result<Self> {
        ensure!(key.len() == 32, "内容密钥长度错误");
        let remaining = expires
            .checked_sub(now())
            .filter(|&n| n > 0)
            .context("授权已到期")?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(remaining))
            .context("授权期限溢出")?;
        Ok(Self {
            file: File::open(path)?,
            header: inspect(path)?,
            key,
            chunk: u64::MAX,
            cache: Vec::new(),
            expires,
            deadline,
        })
    }
    pub fn read_at(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        ensure!(
            now() < self.expires && Instant::now() < self.deadline,
            "授权已到期"
        );
        ensure!(length <= 4 * CHUNK, "读取请求过大");
        let total = number(&self.header.body, "size")?;
        if offset >= total {
            return Ok(Vec::new());
        }
        let end = total.min(offset + length as u64);
        let mut pos = offset;
        let mut output = Vec::with_capacity((end - offset) as usize);
        while pos < end {
            let index = pos / CHUNK as u64;
            if self.chunk != index {
                let n = CHUNK.min((total - index * CHUNK as u64) as usize);
                self.file.seek(SeekFrom::Start(
                    self.header.offset + index * (CHUNK as u64 + 16),
                ))?;
                let mut raw = vec![0; n + 16];
                self.file.read_exact(&mut raw)?;
                let mut nonce = [0; 12];
                nonce[..4].copy_from_slice(&decode(field(&self.header.body, "nonce_prefix")?)?);
                nonce[4..].copy_from_slice(&index.to_be_bytes());
                self.cache.zeroize();
                self.cache = Aes256Gcm::new_from_slice(&self.key)
                    .unwrap()
                    .decrypt(
                        Nonce::from_slice(&nonce),
                        Payload {
                            msg: &raw,
                            aad: &self.header.aad,
                        },
                    )
                    .map_err(|_| anyhow::anyhow!("视频完整性校验失败"))?;
                self.chunk = index;
            }
            let start = (pos % CHUNK as u64) as usize;
            let n = (self.cache.len() - start).min((end - pos) as usize);
            output.extend_from_slice(&self.cache[start..start + n]);
            pos += n as u64;
        }
        Ok(output)
    }
}
static READERS: OnceLock<Mutex<HashMap<u64, Reader>>> = OnceLock::new();
fn readers() -> &'static Mutex<HashMap<u64, Reader>> {
    READERS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn dispatch(op: &str, v: Value) -> Result<Value> {
    if op == "close" {
        readers().lock().unwrap().remove(&number(&v, "handle")?);
        return Ok(json!(true));
    }
    let path = Path::new(field(&v, "path")?);
    let h = inspect(path)?;
    match op {
        "inspect" => Ok(h.body),
        "enroll" => enrollment(&h, &v["device"]),
        "verify" => verify_license(&h, &v["license"], field(&v, "device_id")?),
        "open" => {
            let mut reader = Reader::new(path, decode(field(&v, "key")?)?, number(&v, "expires")?)?;
            reader.read_at(0, 1)?;
            let handle = u64::from_le_bytes(random::<8>()) & 0x7fffffffffffffff;
            readers().lock().unwrap().insert(handle, reader);
            Ok(json!({"handle":handle,"size":h.body["size"]}))
        }
        _ => bail!("未知操作"),
    }
}
#[no_mangle]
pub extern "system" fn Java_org_encryptassets_video_Core_call(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    op: jni::objects::JString,
    input: jni::objects::JString,
) -> jni::sys::jstring {
    let out = (|| -> Result<Value> {
        let op: String = env.get_string(&op)?.into();
        let s: String = env.get_string(&input)?.into();
        dispatch(&op, serde_json::from_str(&s)?)
    })();
    let result = match out {
        Ok(v) => json!({"ok":true,"data":v}),
        Err(e) => json!({"ok":false,"error":format!("{e:#}")}),
    };
    env.new_string(result.to_string())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_org_encryptassets_video_Core_read(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    handle: jni::sys::jlong,
    offset: jni::sys::jlong,
    len: jni::sys::jint,
) -> jni::sys::jbyteArray {
    let result = (|| -> Result<Vec<u8>> {
        ensure!(offset >= 0 && len >= 0, "读取范围错误");
        readers()
            .lock()
            .unwrap()
            .get_mut(&(handle as u64))
            .context("播放会话已关闭")?
            .read_at(offset as u64, len as usize)
    })();
    match result {
        Ok(b) => env
            .byte_array_from_slice(&b)
            .map(|a| a.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            let _ = env.throw_new("java/io/IOException", e.to_string());
            std::ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_video_and_license_roundtrip() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("android-video-{}", sha(&random::<16>())));
        std::fs::create_dir_all(&dir)?;
        let source = dir.join("test.mp4");
        let pkg = dir.join("test.eav");
        let raw: Vec<u8> = (0..CHUNK * 2 + 37).map(|i| (i % 251) as u8).collect();
        std::fs::write(&source, &raw)?;
        let issuer = Issuer::create()?;
        pack(&issuer, &source, &pkg)?;
        let h = inspect(&pkg)?;
        let private = RsaPrivateKey::new(&mut OsRng, 2048)?;
        let public = private.to_public_key().to_public_key_der()?;
        let id = sha(public.as_bytes());
        let req = enrollment(
            &h,
            &json!({"public_key":encode(public.as_bytes()),"model":"synthetic"}),
        )?;
        let lic = issue(&issuer, &pkg, &req, 2, 60)?;
        let b = verify_license(&h, &lic, &id)?;
        ensure!(verify_license(&h, &lic, "other-device").is_err());
        let key = private.decrypt(
            Oaep::new_with_mgf_hash::<Sha256, sha1::Sha1>(),
            &decode(field(&b, "wrapped_key")?)?,
        )?;
        let mut reader = Reader::new(&pkg, key.clone(), now() + 60)?;
        assert_eq!(
            reader.read_at((CHUNK - 13) as u64, CHUNK + 39)?,
            raw[CHUNK - 13..2 * CHUNK + 26]
        );
        assert_eq!(reader.read_at(0, 123)?, raw[..123]);
        assert!(reader.read_at(raw.len() as u64, 99)?.is_empty());
        let mut bad = lic.clone();
        bad["signature"] = json!(encode(&[0u8; 64]));
        ensure!(verify_license(&h, &bad, &id).is_err());
        ensure!(Reader::new(&pkg, key.clone(), now() - 1).is_err());
        drop(reader);
        let mut bytes = std::fs::read(&pkg)?;
        bytes[h.offset as usize + 5] ^= 1;
        std::fs::write(&pkg, &bytes)?;
        let mut corrupted = Reader::new(&pkg, key, now() + 60)?;
        ensure!(corrupted.read_at(0, 10).is_err());
        drop(corrupted);
        let store = dir.join("issuer.key");
        issuer.save(&store, "test-only-passphrase")?;
        ensure!(Issuer::load(&store, "wrong-passphrase").is_err());
        Issuer::load(&store, "test-only-passphrase")?;
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
