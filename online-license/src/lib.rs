use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use p256::{
    ecdsa::{signature::Verifier, Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_ca: Option<String>,
    pub server_url: String,
    pub server_key: String,
    pub group_id: String,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        let u = reqwest::Url::parse(&self.server_url)?;
        ensure!(
            u.scheme() == "https" || (u.scheme() == "http" && u.host_str() == Some("127.0.0.1")),
            "监管地址必须使用 HTTPS（本机测试允许 127.0.0.1）"
        );
        ensure!(
            u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none()
                && u.path() == "/",
            "请输入服务器根地址"
        );
        uuid::Uuid::parse_str(&self.group_id)?;
        VerifyingKey::from_public_key_der(&B64.decode(&self.server_key)?)?;
        Ok(())
    }
}
pub fn client_with_ca(ca: Option<&str>) -> Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder();
    if let Some(pem) = ca {
        ensure!(pem.len() <= 65536, "证书过大");
        builder = builder.add_root_certificate(reqwest::Certificate::from_pem(pem.as_bytes())?);
    }
    Ok(builder
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
pub fn admin(url: &str, token: &str, operation: Option<&Value>) -> Result<Value> {
    admin_with_ca(url, token, operation, None)
}
pub fn admin_with_ca(
    url: &str,
    token: &str,
    operation: Option<&Value>,
    ca: Option<&str>,
) -> Result<Value> {
    let u = reqwest::Url::parse(url)?;
    ensure!(
        u.scheme() == "https" || (u.scheme() == "http" && u.host_str() == Some("127.0.0.1")),
        "管理员连接必须使用 HTTPS"
    );
    let c = client_with_ca(ca)?;
    let endpoint = format!("{}/api/admin", url.trim_end_matches('/'));
    let request = if let Some(v) = operation {
        c.post(endpoint).json(v)
    } else {
        c.get(endpoint)
    };
    let r = request.bearer_auth(token).send()?;
    let status = r.status();
    let v: Value = r.json()?;
    ensure!(
        status.is_success(),
        "{}",
        v["error"].as_str().unwrap_or("服务器操作失败")
    );
    Ok(v)
}
fn ticks() -> u64 {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetTickCount64() -> u64;
        }
        unsafe { GetTickCount64() }
    }
    #[cfg(unix)]
    {
        let mut t = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let r = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut t) };
        if r != 0 {
            return u64::MAX;
        }
        (t.tv_sec as u64).saturating_mul(1000) + (t.tv_nsec as u64) / 1000000
    }
}
#[derive(Clone, Debug)]
pub struct Grant {
    pub activated: i64,
    pub expires: i64,
    pub time: i64,
    deadline: u64,
    allowed: bool,
}
fn request(p: &Policy, content: &str, device: &str, license: &Value) -> Result<Grant> {
    p.validate()?;
    let start = ticks();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let r = client_with_ca(p.server_ca.as_deref())?
        .post(format!("{}/api/lease", p.server_url.trim_end_matches('/')))
        .json(&json!({"content_id":content,"device_id":device,"license":license,"nonce":nonce}))
        .send()?;
    ensure!(
        r.status().is_success(),
        "在线验证失败，请检查网络、组状态和授权"
    );
    let signed: Value = r.json()?;
    let raw = B64.decode(signed["body"].as_str().context("缺少许可")?)?;
    ensure!(raw.len() <= 16384, "服务器响应过大");
    let sig =
        Signature::from_slice(&B64.decode(signed["signature"].as_str().context("缺少签名")?)?)?;
    VerifyingKey::from_public_key_der(&B64.decode(&p.server_key)?)?.verify(&raw, &sig)?;
    let b: Value = serde_json::from_slice(&raw)?;
    let licraw = B64.decode(license["BodyBase64"].as_str().context("LIC 格式无效")?)?;
    let lic: Value = serde_json::from_slice(&licraw)?;
    ensure!(
        b["version"] == 1
            && b["nonce"] == nonce
            && b["content_id"] == content
            && b["device_id"] == device
            && b["group_id"] == p.group_id
            && b["license_id"]
                == *lic
                    .get("LicenseId")
                    .or_else(|| lic.get("license_id"))
                    .context("LIC 编号缺失")?,
        "运行许可绑定无效"
    );
    let time = b["server_time"].as_i64().context("服务器时间无效")?;
    let until = b["lease_until"].as_i64().context("许可期限无效")?;
    let ttl = until.checked_sub(time).context("许可期限无效")?;
    ensure!((0..=120).contains(&ttl), "许可期限超出范围");
    let allowed = b["allowed"].as_bool().context("许可状态无效")?;
    Ok(Grant {
        activated: b["activated_at"].as_i64().context("激活时间无效")?,
        expires: b["expires_at"].as_i64().context("到期时间无效")?,
        time,
        deadline: start.saturating_add(ttl as u64 * 1000),
        allowed,
    })
}
pub struct Lease {
    state: Arc<Mutex<Grant>>,
    stop: Arc<AtomicBool>,
}
impl Lease {
    pub fn start(p: Policy, content: String, device: String, license: Value) -> Result<Self> {
        let grant = request(&p, &content, &device, &license)?;
        ensure!(grant.allowed, "在线授权已暂停或到期");
        ensure!(ticks() < grant.deadline, "运行许可已过期");
        let state = Arc::new(Mutex::new(grant));
        let stop = Arc::new(AtomicBool::new(false));
        let s = state.clone();
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                for _ in 0..30 {
                    std::thread::sleep(Duration::from_secs(1));
                    if flag.load(Ordering::Relaxed) {
                        return;
                    }
                }
                if let Ok(g) = request(&p, &content, &device, &license) {
                    let mut old = s.lock().unwrap();
                    if g.time >= old.time {
                        *old = g
                    } else {
                        old.allowed = false;
                    }
                }
            }
        });
        Ok(Self { state, stop })
    }
    pub fn check(&self) -> Result<Grant> {
        let g = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("在线授权状态异常"))?
            .clone();
        ensure!(g.allowed, "在线授权已暂停或到期");
        ensure!(
            ticks() < g.deadline,
            "无法续期，在线运行许可已过期，请重新联网验证"
        );
        Ok(g)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(windows)]
pub mod settings;
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_ca: Option<String>,
    pub enabled: bool,
    pub url: String,
    pub token: String,
    pub policy: Option<Policy>,
    pub group_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_expiry_and_revocation_fail_closed() {
        let state = Arc::new(Mutex::new(Grant {
            activated: 0,
            expires: 1,
            time: 0,
            deadline: ticks() + 10000,
            allowed: true,
        }));
        let lease = Lease {
            state: state.clone(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        assert!(lease.check().is_ok());
        state.lock().unwrap().deadline = ticks();
        assert!(lease.check().is_err());
        state.lock().unwrap().deadline = ticks() + 10000;
        state.lock().unwrap().allowed = false;
        assert!(lease.check().is_err());
    }
    #[test]
    fn plaintext_remote_endpoint_rejected() {
        let p = Policy {
            server_ca: None,
            server_url: "http://192.0.2.10:7186".into(),
            server_key: "".into(),
            group_id: uuid::Uuid::new_v4().to_string(),
        };
        assert!(p.validate().is_err());
    }
}
