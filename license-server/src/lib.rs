use anyhow::{ensure, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::{DecodePublicKey, EncodePublicKey},
};
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
#[derive(Clone)]
pub struct App {
    db: Arc<Mutex<Connection>>,
    key: Arc<SigningKey>,
    token: Option<Arc<String>>,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().with_context(|| format!("缺少字段 {k}"))
}
fn bounded(v: &Value, k: &str, max: usize) -> Result<String> {
    let s = field(v, k)?.trim();
    ensure!(!s.is_empty() && s.len() <= max, "字段 {k} 长度无效");
    Ok(s.into())
}
fn digest(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}
struct Error(anyhow::Error);
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
type Api = std::result::Result<Json<Value>, Error>;
impl App {
    pub fn open(dir: &Path, token: String) -> Result<Self> {
        ensure!(token.len() >= 32, "管理员令牌至少 32 字符");
        Self::open_with_token(dir, Some(token))
    }
    pub fn open_daily(dir: &Path) -> Result<Self> {
        Self::open_with_token(dir, None)
    }
    fn open_with_token(dir: &Path, token: Option<String>) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("server.key");
        ensure!(
            path.exists() || !dir.join("licenses.sqlite").exists(),
            "数据库已存在但服务器密钥缺失，请恢复原密钥，不能自动更换"
        );
        let key = if path.exists() {
            SigningKey::from_slice(&std::fs::read(path)?)?
        } else {
            let key = SigningKey::random(&mut OsRng);
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options.open(path)?;
            f.write_all(&key.to_bytes())?;
            f.sync_all()?;
            key
        };
        let mut app = Self::from_connection(
            Connection::open(dir.join("licenses.sqlite"))?,
            key,
            String::new(),
        )?;
        app.token = token.map(Arc::new);
        Ok(app)
    }
    fn from_connection(db: Connection, key: SigningKey, token: String) -> Result<Self> {
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
   CREATE TABLE IF NOT EXISTS groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,note TEXT NOT NULL,enabled INTEGER NOT NULL DEFAULT 1);
   CREATE TABLE IF NOT EXISTS contents(id TEXT PRIMARY KEY,group_id TEXT NOT NULL REFERENCES groups(id),name TEXT NOT NULL,platform TEXT NOT NULL,issuer TEXT NOT NULL,policy TEXT NOT NULL,enabled INTEGER NOT NULL DEFAULT 1);
   CREATE TABLE IF NOT EXISTS licenses(id TEXT PRIMARY KEY,content_id TEXT NOT NULL REFERENCES contents(id),device TEXT NOT NULL,hash TEXT NOT NULL,activated INTEGER NOT NULL,expires INTEGER NOT NULL,last_seen INTEGER NOT NULL,enabled INTEGER NOT NULL DEFAULT 1);
   CREATE TABLE IF NOT EXISTS audit(id INTEGER PRIMARY KEY AUTOINCREMENT,time INTEGER NOT NULL,action TEXT NOT NULL,target TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS clock(id INTEGER PRIMARY KEY CHECK(id=1),last INTEGER NOT NULL);")?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            key: Arc::new(key),
            token: Some(Arc::new(token)),
        })
    }
    pub fn use_daily_tokens(&mut self) {
        self.token = None;
    }

    pub fn current_daily_token(&self) -> Result<String> {
        self.daily_token_at(now())
    }

    fn daily_token_at(&self, seconds: i64) -> Result<String> {
        // Beijing 04:00 is 20:00 UTC on the previous calendar day.
        let period = (seconds + 4 * 3600).div_euclid(86400);
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("数据库不可用"))?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS daily_admin(id INTEGER PRIMARY KEY CHECK(id=1), period INTEGER NOT NULL, token TEXT NOT NULL)")?;
        let previous: Option<(i64, String)> = tx
            .query_row(
                "SELECT period,token FROM daily_admin WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((saved, token)) = previous {
            ensure!(saved <= period, "服务器时间回退");
            if saved == period {
                return Ok(token);
            }
        }
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        OsRng.try_fill_bytes(&mut bytes)?;
        let token = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        tx.execute(
            "INSERT OR REPLACE INTO daily_admin(id,period,token) VALUES(1,?,?)",
            params![period, token],
        )?;
        tx.commit()?;
        Ok(token)
    }
    fn auth(&self, h: &HeaderMap) -> Result<()> {
        let got = h
            .get("authorization")
            .and_then(|x| x.to_str().ok())
            .unwrap_or("");
        let token = match &self.token {
            Some(value) => value.to_string(),
            None => self.current_daily_token()?,
        };
        let want = format!("Bearer {token}");
        ensure!(
            bool::from(got.as_bytes().ct_eq(want.as_bytes())),
            "管理员认证失败"
        );
        Ok(())
    }
    fn public(&self) -> Result<String> {
        Ok(B64.encode(self.key.verifying_key().to_public_key_der()?.as_bytes()))
    }
    fn admin(&self, v: Value) -> Result<Value> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("数据库不可用"))?;
        let tx = db.transaction()?;
        let op = field(&v, "op")?;
        let result = match op {
            "create_group" => {
                let id = uuid::Uuid::new_v4().to_string();
                let name = bounded(&v, "name", 160)?;
                let note = v["note"].as_str().unwrap_or("");
                ensure!(note.len() <= 2000, "备注过长");
                tx.execute(
                    "INSERT INTO groups(id,name,note) VALUES(?1,?2,?3)",
                    params![id, name, note],
                )?;
                json!({"id":id})
            }
            "register_content" => {
                let id = bounded(&v, "id", 80)?;
                let group = bounded(&v, "group_id", 80)?;
                let platform = field(&v, "platform")?;
                ensure!(
                    ["Windows", "Linux", "Android"].contains(&platform),
                    "平台无效"
                );
                let enabled: bool =
                    tx.query_row("SELECT enabled FROM groups WHERE id=?1", [&group], |r| {
                        r.get(0)
                    })?;
                ensure!(enabled, "组已暂停");
                let issuer = bounded(&v, "issuer", 2048)?;
                VerifyingKey::from_public_key_der(&B64.decode(&issuer)?)?;
                let name = bounded(&v, "name", 512)?;
                let policy = &v["policy"];
                ensure!(
                    policy["group_id"] == group && policy["server_key"] == self.public()?,
                    "监管配置不匹配"
                );
                ensure!(
                    field(policy, "server_url")?.starts_with("https://")
                        || field(policy, "server_url")?.starts_with("http://127.0.0.1:"),
                    "必须使用 HTTPS"
                );
                let old: Option<(String, String, String)> = tx
                    .query_row(
                        "SELECT group_id,issuer,policy FROM contents WHERE id=?1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                if let Some((g, i, p)) = old {
                    ensure!(
                        g == group && i == issuer && serde_json::from_str::<Value>(&p)? == *policy,
                        "内容编号已登记且配置不同"
                    );
                } else {
                    tx.execute("INSERT INTO contents(id,group_id,name,platform,issuer,policy) VALUES(?1,?2,?3,?4,?5,?6)",params![id,group,name,platform,issuer,policy.to_string()])?;
                }
                json!({"ok":true})
            }
            "set_enabled" => {
                let table = match field(&v, "scope")? {
                    "group" => "groups",
                    "content" => "contents",
                    "license" => "licenses",
                    _ => anyhow::bail!("管理范围无效"),
                };
                let id = bounded(&v, "id", 80)?;
                let enabled = v["enabled"].as_bool().context("状态无效")?;
                ensure!(
                    tx.execute(
                        &format!("UPDATE {table} SET enabled=?1 WHERE id=?2"),
                        params![enabled, id]
                    )? == 1,
                    "记录不存在"
                );
                json!({"ok":true})
            }
            _ => anyhow::bail!("操作无效"),
        };
        tx.execute(
            "INSERT INTO audit(time,action,target) VALUES(?1,?2,?3)",
            params![
                now(),
                format!(
                    "{op} {} {}",
                    v["scope"].as_str().unwrap_or(""),
                    v.get("enabled").map(Value::to_string).unwrap_or_default()
                ),
                v.get("id")
                    .or(result.get("id"))
                    .map(Value::to_string)
                    .unwrap_or_default()
            ],
        )?;
        tx.commit()?;
        Ok(result)
    }
    fn list(&self) -> Result<Value> {
        let db = self.db.lock().unwrap();
        let mut groups = vec![];
        let mut q = db.prepare("SELECT id,name,note,enabled FROM groups ORDER BY rowid DESC")?;
        for row in q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"note":r.get::<_,String>(2)?,"enabled":r.get::<_,bool>(3)?})))?{groups.push(row?);}
        let mut contents = vec![];
        let mut q = db.prepare(
            "SELECT id,group_id,name,platform,enabled FROM contents ORDER BY rowid DESC",
        )?;
        for r in q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"group_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"platform":r.get::<_,String>(3)?,"enabled":r.get::<_,bool>(4)?})))?{contents.push(r?);}
        let mut licenses = vec![];
        let mut q=db.prepare("SELECT id,content_id,device,activated,expires,last_seen,enabled FROM licenses ORDER BY last_seen DESC LIMIT 1000")?;
        for r in q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"content_id":r.get::<_,String>(1)?,"device":r.get::<_,String>(2)?,"activated":r.get::<_,i64>(3)?,"expires":r.get::<_,i64>(4)?,"last_seen":r.get::<_,i64>(5)?,"enabled":r.get::<_,bool>(6)?})))?{licenses.push(r?);}
        let mut audit = vec![];
        let mut q =
            db.prepare("SELECT time,action,target FROM audit ORDER BY id DESC LIMIT 100")?;
        for r in q.query_map([],|r|Ok(json!({"time":r.get::<_,i64>(0)?,"action":r.get::<_,String>(1)?,"target":r.get::<_,String>(2)?})))?{audit.push(r?);}
        Ok(
            json!({"groups":groups,"contents":contents,"licenses":licenses,"audit":audit,"server_key":self.public()?,"server_time":now()}),
        )
    }
    fn lease(&self, v: Value, time: i64) -> Result<Value> {
        let nonce = bounded(&v, "nonce", 128)?;
        ensure!(nonce.len() >= 32, "随机挑战长度不足");
        let content = bounded(&v, "content_id", 80)?;
        let device = bounded(&v, "device_id", 256)?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let last: i64 = tx
            .query_row("SELECT last FROM clock WHERE id=1", [], |r| r.get(0))
            .optional()?
            .unwrap_or(0);
        ensure!(time >= last, "服务器时钟回退，请校准时间");
        tx.execute("INSERT INTO clock(id,last) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET last=excluded.last",[time])?;
        // Persist the high-water clock even if a request is later denied.
        tx.commit()?;
        let tx = db.transaction()?;
        let (group,issuer,policy,enabled,platform):(String,String,String,bool,String)=tx.query_row("SELECT c.group_id,c.issuer,c.policy,(c.enabled AND g.enabled),c.platform FROM contents c JOIN groups g ON g.id=c.group_id WHERE c.id=?1",[&content],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        let license = &v["license"];
        let raw = B64.decode(field(license, "BodyBase64")?)?;
        ensure!(raw.len() <= 32768, "LIC 过大");
        let sig = Signature::from_slice(&B64.decode(field(license, "SignatureBase64")?)?)?;
        VerifyingKey::from_public_key_der(&B64.decode(&issuer)?)?.verify(&raw, &sig)?;
        let source: Value = serde_json::from_slice(&raw)?;
        let b = if platform == "Android" {
            let duration = source["duration"].as_i64().context("安卓授权期限无效")?;
            ensure!(duration > 0 && duration % 86400 == 0, "安卓授权期限无效");
            json!({"LicenseId":source["license_id"],"ContentId":source["content_id"],"DeviceId":source["device_id"],"IssuerFingerprint":digest(&B64.decode(&issuer)?),"Days":duration/86400,"IssuedAtUtcSeconds":source["issued"],"ActivateBeforeUtcSeconds":source["import_deadline"],"Online":source["online"]})
        } else {
            source
        };
        ensure!(
            field(&b, "ContentId")? == content && field(&b, "DeviceId")? == device,
            "设备或内容不匹配"
        );
        ensure!(
            field(&b, "IssuerFingerprint")?.eq_ignore_ascii_case(&digest(&B64.decode(&issuer)?)),
            "发行者不匹配"
        );
        ensure!(
            b["Online"] == serde_json::from_str::<Value>(&policy)?,
            "LIC 未绑定此监管配置"
        );
        let id = field(&b, "LicenseId")?;
        uuid::Uuid::parse_str(id)?;
        let hash = digest(&raw);
        let days = b["Days"].as_i64().context("授权天数无效")?;
        ensure!((1..=36500).contains(&days), "授权天数无效");
        let old: Option<(String, String, String, i64, i64, bool)> = tx
            .query_row(
                "SELECT content_id,device,hash,activated,expires,enabled FROM licenses WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let (activated, expires, allowed) = if let Some((c, d, h, a, e, on)) = old {
            ensure!(c == content && d == device && h == hash, "授权编号冲突");
            (a, e, on && enabled && time < e)
        } else {
            ensure!(enabled, "组或内容已暂停");
            let issued = b["IssuedAtUtcSeconds"].as_i64().context("签发时间无效")?;
            let before = b["ActivateBeforeUtcSeconds"]
                .as_i64()
                .context("首次导入期限无效")?;
            ensure!(time >= issued && time < before, "不在首次导入窗口内");
            let end = time.checked_add(days * 86400).context("期限溢出")?;
            tx.execute("INSERT INTO licenses(id,content_id,device,hash,activated,expires,last_seen) VALUES(?1,?2,?3,?4,?5,?6,?5)",params![id,content,device,hash,time,end])?;
            tx.execute(
                "INSERT INTO audit(time,action,target) VALUES(?1,'activate',?2)",
                params![time, id],
            )?;
            (time, end, true)
        };
        tx.execute(
            "UPDATE licenses SET last_seen=?1 WHERE id=?2",
            params![time, id],
        )?;
        tx.commit()?;
        let body = json!({"version":1,"nonce":nonce,"content_id":content,"device_id":device,"license_id":id,"group_id":group,"allowed":allowed,"server_time":time,"activated_at":activated,"expires_at":expires,"lease_until":if allowed {(time+120).min(expires)}else{time}});
        let bytes = serde_json::to_vec(&body)?;
        let signature: Signature = self.key.sign(&bytes);
        Ok(json!({"body":B64.encode(bytes),"signature":B64.encode(signature.to_bytes())}))
    }
}
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}
async fn list(State(s): State<App>, h: HeaderMap) -> Api {
    s.auth(&h)?;
    Ok(Json(s.list()?))
}
async fn admin(State(s): State<App>, h: HeaderMap, Json(v): Json<Value>) -> Api {
    s.auth(&h)?;
    Ok(Json(s.admin(v)?))
}
async fn lease(State(s): State<App>, Json(v): Json<Value>) -> Api {
    Ok(Json(s.lease(v, now())?))
}
async fn headers(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    for (name,value) in [("cache-control","no-store"),("x-content-type-options","nosniff"),("x-frame-options","DENY"),("referrer-policy","no-referrer"),("content-security-policy","default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'")] {response.headers_mut().insert(axum::http::header::HeaderName::from_static(name),axum::http::HeaderValue::from_static(value));}
    response
}
pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(|| async { "ok" }))
        .route("/api/admin", get(list).post(admin))
        .route("/api/lease", post(lease))
        .layer(axum::middleware::from_fn(headers))
        .layer(DefaultBodyLimit::max(65536))
        .with_state(app)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (App, SigningKey, Value, Value) {
        let app = App::from_connection(
            Connection::open_in_memory().unwrap(),
            SigningKey::random(&mut OsRng),
            "x".repeat(40),
        )
        .unwrap();
        let issuer = SigningKey::random(&mut OsRng);
        let pubkey = issuer.verifying_key().to_public_key_der().unwrap();
        let group = app
            .admin(json!({"op":"create_group","name":"测试组"}))
            .unwrap()["id"]
            .clone();
        let policy = json!({"server_url":"https://license.example.com","server_key":app.public().unwrap(),"group_id":group});
        let content = uuid::Uuid::new_v4().to_string();
        app.admin(json!({"op":"register_content","id":content,"group_id":group,"name":"test.exe","platform":"Windows","issuer":B64.encode(pubkey.as_bytes()),"policy":policy})).unwrap();
        let body = json!({"LicenseId":uuid::Uuid::new_v4().to_string(),"ContentId":content,"DeviceId":"test-device","IssuerFingerprint":digest(pubkey.as_bytes()),"Days":1,"IssuedAtUtcSeconds":1000,"ActivateBeforeUtcSeconds":1100,"Online":policy});
        let request = sign_request(&issuer, &body);
        (app, issuer, body, request)
    }
    fn sign_request(key: &SigningKey, b: &Value) -> Value {
        let raw = serde_json::to_vec(b).unwrap();
        let sig: Signature = key.sign(&raw);
        json!({"content_id":b["ContentId"],"device_id":b["DeviceId"],"nonce":"12345678901234567890123456789012","license":{"BodyBase64":B64.encode(raw),"SignatureBase64":B64.encode(sig.to_bytes())}})
    }
    fn body(v: Value) -> Value {
        serde_json::from_slice(&B64.decode(v["body"].as_str().unwrap()).unwrap()).unwrap()
    }
    #[test]
    fn activation_is_server_owned_and_cannot_reset() {
        let (a, _, _, r) = setup();
        let first = body(a.lease(r.clone(), 1050).unwrap());
        let again = body(a.lease(r, 1200).unwrap());
        assert_eq!(first["activated_at"], 1050);
        assert_eq!(first["expires_at"], again["expires_at"]);
        assert_eq!(again["lease_until"], 1320);
    }
    #[test]
    fn revoke_hierarchy_restore_does_not_override_child() {
        let (a, _, b, r) = setup();
        a.lease(r.clone(), 1050).unwrap();
        let id = b["LicenseId"].clone();
        a.admin(json!({"op":"set_enabled","scope":"license","id":id,"enabled":false}))
            .unwrap();
        a.admin(json!({"op":"set_enabled","scope":"group","id":b["Online"]["group_id"],"enabled":false})).unwrap();
        a.admin(
            json!({"op":"set_enabled","scope":"group","id":b["Online"]["group_id"],"enabled":true}),
        )
        .unwrap();
        assert_eq!(body(a.lease(r.clone(), 1060).unwrap())["allowed"], false);
        a.admin(json!({"op":"set_enabled","scope":"license","id":id,"enabled":true}))
            .unwrap();
        assert_eq!(body(a.lease(r, 1070).unwrap())["allowed"], true);
    }
    #[test]
    fn rejects_forgery_policy_strip_and_wrong_device() {
        let (a, key, mut b, r) = setup();
        let mut wrong = r.clone();
        wrong["device_id"] = json!("another");
        assert!(a.lease(wrong, 1050).is_err());
        b["Online"] = Value::Null;
        assert!(a.lease(sign_request(&key, &b), 1050).is_err());
        assert!(a
            .lease(sign_request(&SigningKey::random(&mut OsRng), &b), 1050)
            .is_err());
    }
    #[test]
    fn rejects_expired_activation_and_rollback() {
        let (a, _, _, r) = setup();
        assert!(a.lease(r.clone(), 1100).is_err());
        assert!(a.lease(r, 1050).is_err());
    }
    #[test]
    fn lease_signature_nonce_and_expiry() {
        let (a, _, _, r) = setup();
        let response = a.lease(r.clone(), 1050).unwrap();
        let raw = B64.decode(response["body"].as_str().unwrap()).unwrap();
        let sig =
            Signature::from_slice(&B64.decode(response["signature"].as_str().unwrap()).unwrap())
                .unwrap();
        a.key.verifying_key().verify(&raw, &sig).unwrap();
        assert_eq!(body(response)["nonce"], r["nonce"]);
        assert_eq!(body(a.lease(r, 87450).unwrap())["allowed"], false);
    }
    #[tokio::test]
    async fn admin_api_requires_token_and_bounds_requests() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (a, _, _, _) = setup();
        let response = router(a.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.status().is_success());
        let response = router(a)
            .oneshot(
                Request::builder()
                    .uri("/api/admin")
                    .header("authorization", format!("Bearer {}", "x".repeat(40)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    #[test]
    fn real_http_lease_refresh_and_revoke() {
        let app = App::from_connection(
            Connection::open_in_memory().unwrap(),
            SigningKey::random(&mut OsRng),
            "t".repeat(40),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let issuer = SigningKey::random(&mut OsRng);
        let public = issuer.verifying_key().to_public_key_der().unwrap();
        let group = app
            .admin(json!({"op":"create_group","name":"网络验收"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let policy = online_license::Policy {
            server_ca: None,
            server_url: url.clone(),
            server_key: app.public().unwrap(),
            group_id: group.clone(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        app.admin(json!({"op":"register_content","id":id,"group_id":group,"name":"test","platform":"Windows","issuer":B64.encode(public.as_bytes()),"policy":policy})).unwrap();
        let b = json!({"LicenseId":uuid::Uuid::new_v4().to_string(),"ContentId":id,"DeviceId":"fake-device","IssuerFingerprint":digest(public.as_bytes()),"Days":1,"IssuedAtUtcSeconds":now()-2,"ActivateBeforeUtcSeconds":now()+120,"Online":policy});
        let raw = serde_json::to_vec(&b).unwrap();
        let sig: Signature = issuer.sign(&raw);
        let lic =
            json!({"BodyBase64":B64.encode(raw),"SignatureBase64":B64.encode(sig.to_bytes())});
        let cloned = app.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(listener, router(cloned))
                    .with_graceful_shutdown(async {
                        let _ = rx.await;
                    })
                    .await
                    .unwrap();
            });
        });
        let lease = online_license::Lease::start(policy, id, "fake-device".into(), lic).unwrap();
        assert!(lease.check().is_ok());
        app.admin(json!({"op":"set_enabled","scope":"group","id":group,"enabled":false}))
            .unwrap();
        let start = std::time::Instant::now();
        while lease.check().is_ok() && start.elapsed().as_secs() < 40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(lease.check().is_err());
        drop(lease);
        let _ = tx.send(());
        worker.join().unwrap();
    }
}

#[cfg(test)]
mod android_tests {
    use super::*;
    #[test]
    fn android_native_license_is_verified_before_normalization() {
        let app = App::from_connection(
            Connection::open_in_memory().unwrap(),
            SigningKey::random(&mut OsRng),
            "x".repeat(40),
        )
        .unwrap();
        let issuer = SigningKey::random(&mut OsRng);
        let public = B64.encode(
            issuer
                .verifying_key()
                .to_public_key_der()
                .unwrap()
                .as_bytes(),
        );
        let group = app
            .admin(json!({"op":"create_group","name":"Android"}))
            .unwrap()["id"]
            .clone();
        let policy = json!({"server_url":"https://example.com","server_key":app.public().unwrap(),"group_id":group});
        let id = uuid::Uuid::new_v4().simple().to_string();
        app.admin(json!({"op":"register_content","id":id,"group_id":group,"name":"video.eav","platform":"Android","issuer":public,"policy":policy})).unwrap();
        let body = json!({"version":1,"license_id":uuid::Uuid::new_v4().simple().to_string(),"content_id":id,"device_id":"android-test","issued":1000,"import_deadline":1100,"duration":86400,"online":policy});
        let raw = serde_json::to_vec(&body).unwrap();
        let sig: Signature = issuer.sign(&raw);
        let request = json!({"content_id":id,"device_id":"android-test","nonce":"12345678901234567890123456789012","license":{"BodyBase64":B64.encode(raw),"SignatureBase64":B64.encode(sig.to_bytes())}});
        let signed = app.lease(request, 1050).unwrap();
        let grant: Value =
            serde_json::from_slice(&B64.decode(signed["body"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(grant["allowed"], true);
        assert_eq!(grant["expires_at"], 87450);
    }
}
#[cfg(test)]
mod daily_rotation_tests {
    use super::*;
    #[test]
    fn beijing_boundary_restart_rollback_and_admin_access() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let app = App::open_daily(&dir).unwrap();
        let boundary = 86400 - 14400;
        let previous = app.daily_token_at(boundary - 1).unwrap();
        let current = app.daily_token_at(boundary).unwrap();
        assert_ne!(current, previous);
        assert!(app.daily_token_at(boundary - 1).is_err());
        drop(app);
        let app = App::open_daily(&dir).unwrap();
        assert_eq!(current, app.daily_token_at(boundary + 1).unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {current}").parse().unwrap(),
        );
        assert!(app.auth(&headers).is_err());
        let today = app.current_daily_token().unwrap();
        headers.insert("authorization", format!("Bearer {today}").parse().unwrap());
        assert!(app.auth(&headers).is_ok());
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
