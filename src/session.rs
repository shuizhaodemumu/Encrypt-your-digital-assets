use crate::{core::*, platform::*};
use anyhow::{ensure, Result};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};
pub struct Session {
    pub info: PackageInfo,
    pub state: ActivationState,
    pub body: Option<LicenseBody>,
    pub baseline: i64,
    pub started: Instant,
    pub active: bool,
    pub last_saved: i64,
}
pub type Authorization = Arc<Mutex<Session>>;
impl Session {
    pub fn new(info: PackageInfo) -> Self {
        Self {
            info,
            state: ActivationState::default(),
            body: None,
            baseline: now(),
            started: Instant::now(),
            active: false,
            last_saved: 0,
        }
    }
    pub fn validate(&mut self) -> Result<()> {
        self.validate_at(now())
    }
    #[cfg(feature = "watchdog-test-mode")]
    pub fn expire_for_test(&mut self) -> Result<()> {
        let end = self
            .state
            .current
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("未激活")))?
            .expires_at_utc_seconds;
        self.validate_at(end)
    }
    fn validate_at(&mut self, t: i64) -> Result<()> {
        ensure!(
            self.active && !self.state.locked,
            crate::i18n::text("授权已停止")
        );
        let r = self
            .state
            .current
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("未激活")))?;
        let b = self
            .body
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("未激活")))?;
        ensure!(
            t >= self.baseline + self.started.elapsed().as_secs() as i64 - 2,
            crate::i18n::text("检测到时间回拨")
        );
        check(r, b, t)?;
        ensure!(
            self.baseline + (self.started.elapsed().as_secs() as i64) < r.expires_at_utc_seconds,
            crate::i18n::text("使用期限已结束")
        );
        if let Some(r) = self.state.current.as_mut() {
            r.last_seen_utc_seconds = r.last_seen_utc_seconds.max(t);
        }
        Ok(())
    }
    pub fn resume(&mut self, device: &Device) -> Result<Option<Vec<u8>>> {
        self.state = load_state(self.info.content_id)?;
        if self.state.current.is_none() {
            return Ok(None);
        }
        ensure!(
            !self.state.locked,
            crate::i18n::text("本地授权已锁定，请申请新授权")
        );
        let r = self.state.current.as_ref().unwrap();
        let b = verify(&r.license, &self.info, &device.id)?;
        check(r, &b, now())?;
        let key = device.unwrap(&b.wrapped_content_key)?.to_vec();
        self.body = Some(b);
        self.baseline = now();
        self.started = Instant::now();
        self.active = true;
        self.save()?;
        Ok(Some(key))
    }
    pub fn import(&mut self, path: &Path, device: &Device, exe: &Path) -> Result<Vec<u8>> {
        let s: SignedLicense = read_json(path)?;
        let b = verify(&s, &self.info, &device.id)?;
        let mut old = load_state(self.info.content_id).unwrap_or(ActivationState {
            current: None,
            used: vec![],
            locked: true,
        });
        if old.used.contains(&b.license_id) {
            ensure!(!old.locked, crate::i18n::text("此授权已使用，不能重置期限"));
            let r = old
                .current
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("授权已使用")))?;
            ensure!(
                verify(&r.license, &self.info, &device.id)?.license_id == b.license_id,
                "此授权已使用"
            );
            check(r, &b, now())?;
        } else {
            let r = activate(s, &b, now())?;
            old.used.push(b.license_id);
            old.current = Some(r);
            old.locked = false;
        }
        let key = device.unwrap(&b.wrapped_content_key)?.to_vec();
        ensure!(key.len() == 32, crate::i18n::text("密钥无效"));
        let mut reader = Reader::new(Package::open_exe(exe)?, key.clone())?;
        let mut probe = [0u8; 1];
        reader.read(&mut probe)?;
        self.state = old;
        self.body = Some(b);
        self.baseline = now();
        self.started = Instant::now();
        self.active = true;
        self.save()?;
        Ok(key)
    }
    pub fn save(&mut self) -> Result<()> {
        save_state(self.info.content_id, &self.state)?;
        self.last_saved = now();
        Ok(())
    }
    pub fn lock(&mut self) {
        self.active = false;
        self.state.locked = true;
        let _ = self.save();
    }
}
pub fn authorized(a: &Option<Authorization>) -> Result<()> {
    if let Some(a) = a {
        a.lock()
            .map_err(|_| anyhow::anyhow!(crate::i18n::text("授权线程状态异常")))?
            .validate()?;
    }
    Ok(())
}
