use anyhow::{ensure, Result};
use std::{
    collections::VecDeque,
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
pub fn fault_count(events: &mut VecDeque<u64>, now: u64) -> usize {
    while events.front().is_some_and(|t| now.saturating_sub(*t) > 600) {
        events.pop_front();
    }
    events.push_back(now);
    events.len()
}
pub fn supervise(exe: &Path) -> Result<()> {
    let runtime = tempfile::Builder::new()
        .prefix("encrypt-assets-watch-")
        .tempdir()?;
    let heartbeat = runtime.path().join("heartbeat");
    let start = Instant::now();
    let mut failures = VecDeque::new();
    loop {
        fs::write(&heartbeat, b"starting")?;
        let mut child = Command::new(exe)
            .arg("--viewer-child")
            .env("ENCRYPT_ASSETS_HEARTBEAT", &heartbeat)
            .process_group(0)
            .spawn()?;
        let launched = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break Some(s);
            }
            let age = fs::metadata(&heartbeat)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(Duration::MAX);
            if launched.elapsed() > Duration::from_secs(120) && age > Duration::from_secs(45) {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGTERM);
                }
                std::thread::sleep(Duration::from_secs(2));
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        if status.is_some_and(|s| s.success()) {
            return Ok(());
        }
        let count = fault_count(&mut failures, start.elapsed().as_secs());
        eprintln!("Playback fault restart {count}/3 in ten minutes");
        if count >= 3 {
            #[cfg(feature = "watchdog-test-mode")]
            anyhow::bail!("TEST_ONLY_REBOOT_SUPPRESSED: third fault in ten minutes");
            #[cfg(not(feature = "watchdog-test-mode"))]
            {
                let result = Command::new("systemctl")
                    .args(["reboot", "--no-ask-password"])
                    .status()?;
                ensure!(
                    result.success(),
                    "三次播放故障；系统拒绝重启。已停止重试，请检查内容或系统权限。"
                );
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
pub fn heartbeat() {
    if let Some(path) = std::env::var_os("ENCRYPT_ASSETS_HEARTBEAT") {
        let _ = fs::write(path, b"responsive");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn faults_are_rolling_and_pause_is_not_an_event() {
        let mut e = VecDeque::new();
        assert_eq!(fault_count(&mut e, 0), 1);
        assert_eq!(fault_count(&mut e, 100), 2);
        assert_eq!(fault_count(&mut e, 600), 3);
        assert_eq!(fault_count(&mut e, 701), 2);
        assert_eq!(fault_count(&mut e, 1400), 1);
    }
}
