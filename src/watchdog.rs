use crate::{core::*, platform};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_TIMEOUT},
    System::{Registry::*, Threading::*},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Health {
    Running,
    Paused,
    Stopped,
    Fault,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    pub token: String,
    pub utc: i64,
    pub health: Health,
    pub media: bool,
    pub state: i32,
    pub position: i64,
}
fn path(info: &PackageInfo, token: &str) -> Result<PathBuf> {
    ensure!(
        token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        crate::i18n::text("运行监测编号无效")
    );
    Ok(platform::root()
        .join("watchdog")
        .join(format!("{}-{token}.dat", info.content_id.simple())))
}
pub struct Client {
    file: PathBuf,
    token: String,
    last: Instant,
    stopped: bool,
}
impl Client {
    pub fn start(exe: &Path, info: &PackageInfo) -> Result<Self> {
        let args: Vec<_> = std::env::args().collect();
        let supplied = args
            .iter()
            .position(|a| a == "--supervised")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let token = supplied
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let file = path(info, &token)?;
        let mut client = Self {
            file,
            token: token.clone(),
            last: Instant::now() - Duration::from_secs(2),
            stopped: false,
        };
        client.pulse(Health::Running, false, 0, 0)?;
        if supplied.is_none() {
            Command::new(exe)
                .args(["--watchdog", &std::process::id().to_string(), &token])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }
        Ok(client)
    }
    pub fn pulse(&mut self, health: Health, media: bool, state: i32, position: i64) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        if self.last.elapsed() < Duration::from_secs(1)
            && health != Health::Fault
            && health != Health::Stopped
        {
            return Ok(());
        }
        let hb = Heartbeat {
            pid: std::process::id(),
            token: self.token.clone(),
            utc: now(),
            health: health.clone(),
            media,
            state,
            position,
        };
        platform::protected_write(&self.file, &encode(&hb)?)?;
        self.last = Instant::now();
        if health == Health::Stopped {
            self.stopped = true;
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        let _ = self.pulse(Health::Stopped, false, 0, 0);
    }
    pub fn fault(&mut self) {
        let _ = self.pulse(Health::Fault, false, 7, 0);
        self.stopped = true;
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default, Serialize, Deserialize)]
struct History {
    restarts: Vec<i64>,
    last_reboot: Option<i64>,
}
#[derive(Debug, PartialEq)]
enum Recovery {
    Restart,
    RestartAndReboot,
}
impl History {
    fn record(&mut self, time: i64) -> Recovery {
        self.restarts.retain(|&t| time >= t && time - t <= 600);
        self.restarts.push(time);
        let can_reboot = self
            .last_reboot
            .is_none_or(|t| time >= t && time - t >= 600);
        if self.restarts.len() >= 3 && can_reboot {
            self.last_reboot = Some(time);
            self.restarts.clear();
            Recovery::RestartAndReboot
        } else {
            Recovery::Restart
        }
    }
}
struct HealthCheck {
    last_position: i64,
    last_advance: Instant,
    was_playing: bool,
}
impl HealthCheck {
    fn new() -> Self {
        Self {
            last_position: -1,
            last_advance: Instant::now(),
            was_playing: false,
        }
    }
    fn failed(&mut self, h: &Heartbeat, stale: bool) -> bool {
        if h.health == Health::Stopped {
            self.last_advance = Instant::now();
            self.was_playing = false;
            return false;
        }
        if h.health == Health::Fault || stale {
            return true;
        }
        if h.health == Health::Paused {
            self.last_advance = Instant::now();
            self.was_playing = false;
            return false;
        }
        if !h.media {
            self.last_advance = Instant::now();
            return false;
        }
        if h.state == 7 {
            return true;
        }
        if h.state == 4 || h.state == 6 {
            self.last_advance = Instant::now();
            self.was_playing = false;
            return false;
        }
        if h.position != self.last_position || !self.was_playing {
            self.last_position = h.position;
            self.last_advance = Instant::now();
            self.was_playing = true;
            return false;
        }
        self.last_advance.elapsed() > Duration::from_secs(10)
    }
}
trait Actions {
    fn persist(&mut self, _history: &History) -> Result<()> {
        Ok(())
    }
    fn restart(&mut self) -> Result<u32>;
    fn reboot(&mut self) -> Result<()>;
}
fn recover(history: &mut History, time: i64, actions: &mut impl Actions) -> Result<u32> {
    let result = history.record(time);
    actions.persist(history)?;
    let pid = actions.restart()?;
    if result == Recovery::RestartAndReboot {
        actions.reboot()?;
    }
    Ok(pid)
}
struct WindowsActions<'a> {
    exe: &'a Path,
    token: &'a str,
    pid: u32,
    history_file: &'a Path,
}
impl Actions for WindowsActions<'_> {
    fn persist(&mut self, history: &History) -> Result<()> {
        platform::protected_write(self.history_file, &encode(history)?)
    }
    fn restart(&mut self) -> Result<u32> {
        stop_owned_process(self.pid, self.exe)?;
        let child = Command::new(self.exe)
            .args(["--supervised", self.token])
            .creation_flags(0x08000000)
            .spawn()?;
        self.pid = child.id();
        audit(
            &self.history_file.with_extension("log"),
            "CONTENT_RESTARTED",
            self.pid,
        )?;
        Ok(self.pid)
    }
    fn reboot(&mut self) -> Result<()> {
        #[cfg(feature = "watchdog-test-mode")]
        {
            audit(
                &self.history_file.with_extension("log"),
                "SIMULATED_SYSTEM_REBOOT",
                self.pid,
            )?;
            Ok(())
        }
        #[cfg(not(feature = "watchdog-test-mode"))]
        {
            audit(
                &self.history_file.with_extension("log"),
                "SYSTEM_REBOOT_REQUESTED",
                self.pid,
            )?;
            let status = Command::new("shutdown.exe")
                .args([
                    "/r",
                    "/t",
                    "0",
                    "/d",
                    "p:4:1",
                    "/c",
                    "授权内容程序十分钟内三次故障重启",
                ])
                .creation_flags(0x08000000)
                .status()?;
            ensure!(status.success(), crate::i18n::text("Windows 拒绝重启请求"));
            Ok(())
        }
    }
}
fn stop_owned_process(pid: u32, exe: &Path) -> Result<()> {
    unsafe {
        let handle = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        );
        if handle != 0 {
            if matches_process(handle, exe) && WaitForSingleObject(handle, 0) == WAIT_TIMEOUT {
                let success = TerminateProcess(handle, 2);
                if success == 0 {
                    CloseHandle(handle);
                    anyhow::bail!(crate::i18n::text("无法停止故障内容程序"));
                }
                WaitForSingleObject(handle, 5000);
            }
            CloseHandle(handle);
        }
    }
    Ok(())
}
fn audit(path: &Path, event: &str, pid: u32) -> Result<()> {
    use std::io::Write;
    fs::create_dir_all(path.parent().unwrap())?;
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(log, "{} {event} PID={pid}", now())?;
    Ok(())
}
unsafe fn matches_process(handle: isize, exe: &Path) -> bool {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) == 0 {
        return false;
    }
    let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]));
    fs::canonicalize(path)
        .ok()
        .zip(fs::canonicalize(exe).ok())
        .is_some_and(|(a, b)| {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        })
}
pub(crate) fn alive(pid: u32, exe: &Path) -> bool {
    unsafe {
        let handle = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if handle == 0 {
            return false;
        }
        let running =
            matches_process(handle, exe) && WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        running
    }
}
pub fn supervise(exe: &Path, info: &PackageInfo, mut pid: u32, token: &str) -> Result<()> {
    let file = path(info, token)?;
    let history_path = platform::root()
        .join("watchdog")
        .join(format!("{}-restarts.dat", info.content_id.simple()));
    let mut history: History = if history_path.exists() {
        decode(&platform::dpapi(&fs::read(&history_path)?, false)?, 65536)?
    } else {
        History::default()
    };
    let mut health = HealthCheck::new();
    let mut starting = Instant::now();
    let mut authorized = false;
    let mut last_heartbeat = Instant::now();
    let mut seen_utc = None;
    let mut last_poll = Instant::now();
    loop {
        if last_poll.elapsed() > Duration::from_secs(2) {
            last_heartbeat = Instant::now();
            health = HealthCheck::new();
        }
        last_poll = Instant::now();
        if unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows_sys::Win32::UI::WindowsAndMessaging::SM_SHUTTINGDOWN,
            )
        } != 0
        {
            break;
        }
        let heartbeat = fs::read(&file)
            .ok()
            .and_then(|b| platform::dpapi(&b, false).ok())
            .and_then(|b| decode::<Heartbeat>(&b, 65536).ok())
            .filter(|h| h.pid == pid && h.token == token);
        let running = alive(pid, exe);
        let mut failed = false;
        if let Some(h) = heartbeat {
            if seen_utc != Some(h.utc) {
                seen_utc = Some(h.utc);
                last_heartbeat = Instant::now();
            }
            if h.health == Health::Stopped {
                break;
            }
            authorized = true;
            if !running {
                failed = true;
            } else {
                failed = health.failed(&h, last_heartbeat.elapsed() > Duration::from_secs(10));
            }
        } else if authorized {
            failed = !running || last_heartbeat.elapsed() > Duration::from_secs(10);
        } else if !running || starting.elapsed() > Duration::from_secs(45) {
            break;
        }
        if failed {
            // Never recover expired/locked/invalid authorization, even after a crash.
            let device = match platform::Device::for_info(info) {
                Ok(device) => device,
                Err(_) => {
                    disable_startup(info)?;
                    stop_owned_process(pid, exe)?;
                    break;
                }
            };
            let mut session = crate::session::Session::new(info.clone());
            match session.resume(&device) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    disable_startup(info)?;
                    stop_owned_process(pid, exe)?;
                    break;
                }
                Err(_) => {
                    if session.state.current.is_some() {
                        session.lock();
                    }
                    disable_startup(info)?;
                    stop_owned_process(pid, exe)?;
                    break;
                }
            }
            let mut actions = WindowsActions {
                exe,
                token,
                pid,
                history_file: &history_path,
            };
            pid = recover(&mut history, now(), &mut actions)?;
            health = HealthCheck::new();
            starting = Instant::now();
            authorized = false;
            last_heartbeat = Instant::now();
            seen_utc = None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = fs::remove_file(file);
    Ok(())
}

fn startup_name(info: &PackageInfo) -> String {
    format!(
        "{}-{}",
        crate::platform::STATE_NAMESPACE,
        info.content_id.simple()
    )
}
pub fn enable_startup(exe: &Path, info: &PackageInfo) -> Result<PathBuf> {
    let target = crate::install::install_content_for_startup(exe, info)?;
    let command = format!("\"{}\" --autostart", target.display());
    ensure!(
        command.encode_utf16().count() <= 260,
        "自动启动路径过长，请选择较短的用户数据目录"
    );
    let data = platform::wide(&command);
    unsafe {
        let mut key = 0;
        ensure!(
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                platform::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut()
            ) == 0,
            crate::i18n::text("无法设置登录自动启动")
        );
        // A process started by Run should not rewrite its own unchanged Run
        // value. Validate existing registration, then update only if needed.
        let mut existing = vec![0u16; data.len()];
        let mut existing_bytes = (existing.len() * 2) as u32;
        let mut existing_kind = 0;
        if RegQueryValueExW(
            key,
            platform::wide(&startup_name(info)).as_ptr(),
            std::ptr::null_mut(),
            &mut existing_kind,
            existing.as_mut_ptr().cast(),
            &mut existing_bytes,
        ) == 0
            && existing_kind == REG_SZ
            && existing == data
        {
            RegCloseKey(key);
            return Ok(target);
        }
        let result = RegSetValueExW(
            key,
            platform::wide(&startup_name(info)).as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        );
        if result != 0 {
            RegCloseKey(key);
            anyhow::bail!("无法写入登录启动项：{}", result);
        }
        let mut actual = vec![0u16; data.len()];
        let mut bytes = (actual.len() * 2) as u32;
        let mut kind = 0;
        let verified = RegQueryValueExW(
            key,
            platform::wide(&startup_name(info)).as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            actual.as_mut_ptr().cast(),
            &mut bytes,
        );
        RegCloseKey(key);
        ensure!(
            verified == 0 && kind == REG_SZ && actual == data,
            "登录启动项写入后校验失败"
        );
    }
    Ok(target)
}
pub fn disable_startup(info: &PackageInfo) -> Result<()> {
    unsafe {
        let mut key = 0;
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            platform::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        ) == 0
        {
            RegDeleteValueW(key, platform::wide(&startup_name(info)).as_ptr());
            RegCloseKey(key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        restarts: u32,
        reboots: u32,
    }
    impl Actions for Fake {
        fn restart(&mut self) -> Result<u32> {
            self.restarts += 1;
            Ok(self.restarts)
        }
        fn reboot(&mut self) -> Result<()> {
            self.reboots += 1;
            Ok(())
        }
    }
    #[test]
    fn three_faults_reboot_without_countdown() {
        let mut h = History::default();
        let mut a = Fake::default();
        recover(&mut h, 1000, &mut a).unwrap();
        recover(&mut h, 1200, &mut a).unwrap();
        assert_eq!(a.reboots, 0);
        recover(&mut h, 1599, &mut a).unwrap();
        assert_eq!((a.restarts, a.reboots), (3, 1));
        recover(&mut h, 1600, &mut a).unwrap();
        recover(&mut h, 1601, &mut a).unwrap();
        recover(&mut h, 1602, &mut a).unwrap();
        assert_eq!(a.reboots, 1);
    }
    #[test]
    fn failures_outside_window_do_not_escalate() {
        let mut h = History::default();
        let mut a = Fake::default();
        for t in [1000, 1601, 2202] {
            recover(&mut h, t, &mut a).unwrap();
        }
        assert_eq!(a.reboots, 0);
    }
    #[test]
    fn static_content_pause_close_and_fault() {
        let mut check = HealthCheck::new();
        let mut h = Heartbeat {
            pid: 1,
            token: "a".repeat(32),
            utc: 1000,
            health: Health::Running,
            media: false,
            state: 0,
            position: 0,
        };
        assert!(!check.failed(&h, false));
        h.health = Health::Paused;
        assert!(!check.failed(&h, false));
        assert!(check.failed(&h, true));
        h.health = Health::Stopped;
        assert!(!check.failed(&h, true));
        h.health = Health::Running;
        assert!(check.failed(&h, true));
        h.health = Health::Fault;
        assert!(check.failed(&h, false));
    }
}
