use crate::{core::*, platform};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_ca: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub explicit_offline: bool,
    pub url: String,
    pub token: String,
    pub policy: Option<online_license::Policy>,
    pub group_name: String,
}
pub fn load() -> Result<Config> {
    let path = platform::root().join("online-admin.bin");
    if !path.exists() {
        return Ok(Config::default());
    }
    Ok(serde_json::from_slice(&platform::dpapi(
        &std::fs::read(path)?,
        false,
    )?)?)
}
pub fn save(c: &Config) -> Result<()> {
    platform::protected_write(
        &platform::root().join("online-admin.bin"),
        &serde_json::to_vec(c)?,
    )
}
pub fn ready() -> bool {
    load()
        .map(|c| {
            if c.enabled {
                c.policy.is_some()
            } else {
                c.explicit_offline
            }
        })
        .unwrap_or(false)
}
pub fn selected_policy() -> Result<Option<online_license::Policy>> {
    let c = load()?;
    if !c.enabled {
        return Ok(None);
    }
    let p = c.policy.context("联网模式：请先创建或选择组")?;
    p.validate()?;
    Ok(Some(p))
}
pub fn list(c: &Config) -> Result<Value> {
    online_license::admin_with_ca(&c.url, &c.token, None, c.server_ca.as_deref())
}
pub fn create(c: &Config, name: &str) -> Result<Value> {
    admin(
        &c.url,
        &c.token,
        Some(&json!({"op":"create_group","name":name,"note":""})),
    )
}
pub fn register(info: &PackageInfo) -> Result<()> {
    let Some(p) = &info.online else { return Ok(()) };
    let c = load()?;
    ensure!(
        c.url.trim_end_matches('/') == p.server_url.trim_end_matches('/'),
        "管理器地址与内容不符"
    );
    admin(
        &c.url,
        &c.token,
        Some(
            &json!({"op":"register_content","id":info.content_id.to_string(),"group_id":p.group_id,"name":info.display_name,"platform":"Windows","issuer":info.issuer_public_key,"policy":p}),
        ),
    )?;
    Ok(())
}

pub fn signing_time(info: &PackageInfo, fallback: i64) -> Result<i64> {
    if info.online.is_none() {
        return Ok(fallback);
    }
    let c = load()?;
    ensure!(
        c.url.trim_end_matches('/')
            == info
                .online
                .as_ref()
                .unwrap()
                .server_url
                .trim_end_matches('/'),
        "请连接内容所属监管服务器"
    );
    list(&c)?["server_time"].as_i64().context("服务器时间无效")
}

fn admin(url: &str, token: &str, operation: Option<&Value>) -> Result<Value> {
    let c = load()?;
    online_license::admin_with_ca(url, token, operation, c.server_ca.as_deref())
}
