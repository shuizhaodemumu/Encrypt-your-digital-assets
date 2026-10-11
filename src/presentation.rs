use crate::{
    automation::{Apartment, Dispatch, Value},
    core::*,
    platform,
    session::Authorization,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};

pub fn supported(extension: &str) -> bool {
    matches!(extension, "ppt" | "pptx" | "pps" | "ppsx")
}
pub fn available() -> bool {
    Dispatch::registered("PowerPoint.Application")
}
pub fn validate(path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    ensure!(
        supported(&ext),
        crate::i18n::text("仅支持 PPT/PPTX/PPS/PPSX 放映，宏格式不支持")
    );
    ensure!(
        fs::metadata(path)?.len() > 0,
        crate::i18n::text("PPT 文件为空")
    );
    if ext.ends_with('x') {
        let mut zip = zip::ZipArchive::new(fs::File::open(path)?)
            .context(crate::i18n::text("PPTX 不是有效 ZIP 文件"))?;
        ensure!(
            zip.by_name("[Content_Types].xml").is_ok()
                && zip.by_name("ppt/presentation.xml").is_ok(),
            crate::i18n::text("PPTX 结构无效")
        );
        ensure!(
            zip.by_name("ppt/vbaProject.bin").is_err(),
            crate::i18n::text("不接受带 VBA 宏的演示文件")
        );
    } else {
        let mut magic = [0; 8];
        fs::File::open(path)?.read_exact(&mut magic)?;
        ensure!(
            magic == [208, 207, 17, 224, 161, 177, 26, 225],
            crate::i18n::text("PPT/PPS 文件头无效")
        );
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    parent_pid: u32,
    parent_exe: PathBuf,
    extension: String,
}
#[derive(Serialize, Deserialize)]
struct Lease {
    token: String,
    sequence: u64,
    until: i64,
    stop: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum State {
    Starting,
    Showing,
    Ended,
    Fault,
}
#[derive(Clone, Serialize, Deserialize)]
struct Report {
    token: String,
    state: State,
    hwnd: isize,
    pid: u32,
    message: String,
    updated: i64,
}
fn protected<T: Serialize>(file: &Path, value: &T) -> Result<()> {
    atomic(file, &platform::dpapi(&encode(value)?, true)?)
}
fn load<T: serde::de::DeserializeOwned>(file: &Path) -> Result<T> {
    ensure!(
        fs::metadata(file)?.len() <= 65536,
        crate::i18n::text("演示控制文件过大")
    );
    decode(&platform::dpapi(&fs::read(file)?, false)?, 65536)
}
fn root() -> PathBuf {
    platform::root().join("powerpoint")
}
fn checked_dir(dir: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(dir)?;
    let name = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .context(crate::i18n::text("目录名无效"))?;
    uuid::Uuid::parse_str(name).context(crate::i18n::text("演示目录编号无效"))?;
    // Packaged Windows apps may overlay AppData: the root handle and a child handle
    // can resolve to different physical parents. Resolve the exact expected child,
    // rather than comparing the independently resolved parent directory.
    let expected = root().join(name);
    ensure!(
        fs::canonicalize(&expected)? == canonical,
        crate::i18n::text("演示临时目录越界")
    );
    ensure!(
        !fs::symlink_metadata(&expected)?.file_type().is_symlink()
            && !fs::symlink_metadata(dir)?.file_type().is_symlink(),
        crate::i18n::text("演示目录不能是链接")
    );
    Ok(canonical)
}
fn cleanup(dir: &Path) {
    if checked_dir(dir).is_err() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&p) {
                if meta.file_type().is_file() {
                    let mut permissions = meta.permissions();
                    permissions.set_readonly(false);
                    let _ = fs::set_permissions(&p, permissions);
                    let _ = fs::remove_file(p);
                }
            }
        }
    }
    let _ = fs::remove_dir(dir);
}
fn property(token: &str) -> Vec<u16> {
    platform::wide(&format!("{}-PPT-{token}", platform::STATE_NAMESPACE))
}
fn owned_window(hwnd: isize, pid: u32, token: &str) -> bool {
    unsafe {
        if hwnd == 0 || IsWindow(hwnd) == 0 || GetPropW(hwnd, property(token).as_ptr()) == 0 {
            return false;
        }
        let mut actual = 0;
        GetWindowThreadProcessId(hwnd, &mut actual);
        actual == pid
    }
}
fn close_window(hwnd: isize, pid: u32, token: &str) {
    if owned_window(hwnd, pid, token) {
        unsafe {
            let mut class = [0u16; 128];
            let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
            if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "screenClass" {
                // Hiding then WM_CLOSE can wedge Office's slideshow message loop.
                // Esc ends only this slideshow, just like normal user interaction.
                PostMessageW(hwnd, WM_KEYDOWN, 27, 0);
                PostMessageW(hwnd, WM_KEYUP, 27, 0);
            } else {
                ShowWindowAsync(hwnd, SW_HIDE);
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }
}
fn slideshow_windows(file_name: &str) -> Vec<isize> {
    struct Search<'a> {
        name: &'a str,
        windows: Vec<isize>,
    }
    unsafe extern "system" fn visit(hwnd: isize, data: isize) -> i32 {
        let search = &mut *(data as *mut Search<'_>);
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "screenClass" {
            let mut title = [0u16; 2048];
            let n = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if String::from_utf16_lossy(&title[..n.max(0) as usize]).contains(search.name) {
                search.windows.push(hwnd);
            }
        }
        1
    }
    let mut search = Search {
        name: file_name,
        windows: vec![],
    };
    unsafe {
        EnumWindows(Some(visit), &mut search as *mut _ as isize);
    }
    search.windows
}
pub struct Presentation {
    child: Child,
    dir: PathBuf,
    token: String,
    sequence: u64,
    last: Instant,
    started: Instant,
    report: Option<Report>,
    stopped: bool,
}
impl Presentation {
    pub fn open(
        mut reader: Reader,
        auth: Option<Authorization>,
        info: &PackageInfo,
    ) -> Result<Self> {
        if let Some(a) = &auth {
            a.lock().unwrap().validate()?;
        }
        ensure!(available(),crate::i18n::text("此 PPT 需要接收端安装并激活 Microsoft PowerPoint 桌面版；WPS、网页版或 LibreOffice 不能替代此播放方式。"));
        let ext = Path::new(&info.display_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        ensure!(supported(&ext), crate::i18n::text("PPT 包扩展名无效"));
        if let Some(a) = &auth {
            a.lock().unwrap().validate()?;
        }
        fs::create_dir_all(root())?;
        let dir = root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&dir)?;
        let result = (|| {
            let token = uuid::Uuid::new_v4().simple().to_string();
            let content = dir.join(format!("content-{token}.{ext}"));
            let mut file = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&content)?;
            let mut buffer = zeroize::Zeroizing::new(vec![0u8; CHUNK]);
            loop {
                if let Some(a) = &auth {
                    a.lock().unwrap().validate()?;
                }
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
            }
            file.sync_all()?;
            drop(file);
            validate(&content)?;
            let mut permissions = fs::metadata(&content)?.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&content, permissions)?;
            if let Some(a) = &auth {
                a.lock().unwrap().validate()?;
            }
            let request = Request {
                token: token.clone(),
                parent_pid: std::process::id(),
                parent_exe: std::env::current_exe()?,
                extension: ext,
            };
            protected(&dir.join("request.dat"), &request)?;
            protected(
                &dir.join("lease.dat"),
                &Lease {
                    token: token.clone(),
                    sequence: 0,
                    until: now() + 8,
                    stop: false,
                },
            )?;
            let child = Command::new(std::env::current_exe()?)
                .arg("--ppt-host")
                .arg(&dir)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(Self {
                child,
                dir: dir.clone(),
                token,
                sequence: 0,
                last: Instant::now(),
                started: Instant::now(),
                report: None,
                stopped: false,
            })
        })();
        if result.is_err() {
            cleanup(&dir);
        }
        result
    }
    pub fn poll(&mut self) -> Result<State> {
        if self.stopped {
            return Ok(State::Ended);
        }
        if self.last.elapsed() >= Duration::from_millis(500) {
            self.sequence += 1;
            protected(
                &self.dir.join("lease.dat"),
                &Lease {
                    token: self.token.clone(),
                    sequence: self.sequence,
                    until: now() + 8,
                    stop: false,
                },
            )?;
            self.last = Instant::now();
        }
        if let Ok(report) = load::<Report>(&self.dir.join("report.dat")) {
            ensure!(
                report.token == self.token,
                crate::i18n::text("演示状态编号不匹配")
            );
            self.report = Some(report);
        }
        let exit = self.child.try_wait()?;
        if let Some(report) = &self.report {
            match report.state {
                State::Fault => anyhow::bail!(crate::tr_format!(
                    "PowerPoint 放映失败：{}",
                    "PowerPoint slide show failed: {}",
                    report.message
                )),
                State::Ended => return Ok(State::Ended),
                State::Showing => {
                    if exit.is_some() {
                        anyhow::bail!(crate::i18n::text("PowerPoint 放映控制进程异常退出"));
                    }
                    if now() - report.updated > 12 {
                        anyhow::bail!(crate::i18n::text("PowerPoint 放映控制进程未响应"));
                    }
                    return Ok(State::Showing);
                }
                State::Starting => {}
            }
        }
        if exit.is_some() {
            anyhow::bail!(crate::i18n::text(
                "PowerPoint 未能启动，请检查安装、激活和文件权限"
            ));
        }
        ensure!(
            self.started.elapsed() < Duration::from_secs(75),
            crate::i18n::text("PowerPoint 启动超时，请检查是否有激活或文件密码提示")
        );
        Ok(State::Starting)
    }
    pub fn close(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = protected(
            &self.dir.join("lease.dat"),
            &Lease {
                token: self.token.clone(),
                sequence: self.sequence + 1,
                until: now(),
                stop: true,
            },
        );
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(4) {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            if let Some(report) = &self.report {
                close_window(report.hwnd, report.pid, &self.token);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        cleanup(&self.dir);
    }
}
impl Drop for Presentation {
    fn drop(&mut self) {
        self.close();
    }
}
struct Show {
    app: Dispatch,
    pres: Dispatch,
    window: Option<Dispatch>,
    hwnd: isize,
    pid: u32,
    token: String,
}
impl Show {
    fn open(file: &Path, token: &str, stop: &AtomicBool) -> Result<Self> {
        let app = Dispatch::create("PowerPoint.Application", true)?;
        let security = app.get("AutomationSecurity")?.number()?;
        app.set("AutomationSecurity", Value::integer(3))?;
        let opened = (|| {
            // Office's file APIs do not accept Rust's canonical \\?\ path prefix.
            let canonical = file.to_string_lossy();
            let office_path = if let Some(unc) = canonical.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{unc}")
            } else {
                canonical
                    .strip_prefix(r"\\?\")
                    .unwrap_or(&canonical)
                    .to_owned()
            };
            app.get("Presentations")?
                .object()?
                .method(
                    "Open",
                    vec![
                        Value::string(&office_path)?,
                        Value::integer(-1),
                        Value::integer(0),
                        Value::integer(0),
                    ],
                )?
                .object()
        })();
        let restored = app.set("AutomationSecurity", Value::integer(security));
        let pres = opened?;
        let mut owner = Self {
            app,
            pres,
            window: None,
            hwnd: 0,
            pid: 0,
            token: token.into(),
        };
        restored?;
        ensure!(!stop.load(Ordering::SeqCst), "授权控制已停止");
        ensure!(
            owner.pres.get("ReadOnly")?.number()? != 0,
            "PowerPoint 没有以只读方式打开"
        );
        let settings = owner.pres.get("SlideShowSettings")?.object()?;
        settings.set("ShowType", Value::integer(1))?;
        settings.set("ShowPresenterView", Value::integer(0))?;
        owner.window = Some(settings.method("Run", vec![])?.object()?);
        ensure!(!stop.load(Ordering::SeqCst), "授权控制已停止");
        // SlideShowWindow.HWND is restricted and unavailable through IDispatch in Office 16.
        // Match the unique token-bearing filename, never the first PowerPoint window.
        let name = file
            .file_name()
            .context("演示文件名无效")?
            .to_string_lossy();
        let deadline = Instant::now();
        let hwnd = loop {
            let windows = slideshow_windows(&name);
            ensure!(windows.len() <= 1, "无法唯一识别本次放映窗口");
            if let Some(hwnd) = windows.first() {
                break *hwnd;
            }
            ensure!(!stop.load(Ordering::SeqCst), "授权控制已停止");
            ensure!(
                deadline.elapsed() < Duration::from_secs(5),
                "未找到本次放映窗口"
            );
            Apartment::pump();
            std::thread::sleep(Duration::from_millis(50));
        };
        unsafe {
            ensure!(IsWindow(hwnd) != 0, "PowerPoint 未建立放映窗口");
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            ensure!(pid != 0, "放映窗口进程无效");
            owner.hwnd = hwnd;
            owner.pid = pid;
            ensure!(
                SetPropW(hwnd, property(token).as_ptr(), std::process::id() as isize) != 0,
                "无法绑定本次放映窗口"
            );
            SetForegroundWindow(hwnd);
        }
        Ok(owner)
    }
}
impl Drop for Show {
    fn drop(&mut self) {
        if let Some(window) = self.window.take() {
            if let Ok(view) = window.get("View").and_then(Value::object) {
                let _ = view.method("Exit", vec![]);
            }
        }
        close_window(self.hwnd, self.pid, &self.token);
        let _ = self.pres.set("Saved", Value::integer(-1));
        let _ = self.pres.method("Close", vec![]);
        // Quit only if no presentations remain. Never terminate POWERPNT.EXE or close unrelated files.
        if self
            .app
            .get("Presentations")
            .and_then(Value::object)
            .and_then(|p| p.get("Count"))
            .and_then(|v| v.number())
            .ok()
            == Some(0)
        {
            let _ = self.app.method("Quit", vec![]);
        }
    }
}
fn lease_valid(dir: &Path, request: &Request) -> bool {
    load::<Lease>(&dir.join("lease.dat")).is_ok_and(|l| {
        l.token == request.token && !l.stop && l.until >= now() && l.until <= now() + 10
    })
}
pub fn host(dir: &Path) -> Result<()> {
    let dir = checked_dir(dir)?;
    let request: Request = load(&dir.join("request.dat"))?;
    ensure!(
        request.token.len() == 32
            && request.token.bytes().all(|c| c.is_ascii_hexdigit())
            && supported(&request.extension),
        "演示请求无效"
    );
    ensure!(
        crate::watchdog::alive(request.parent_pid, &request.parent_exe)
            && lease_valid(&dir, &request),
        "演示请求已停止"
    );
    let token = request.token.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let hwnd = Arc::new(AtomicIsize::new(0));
    let pid = Arc::new(AtomicU32::new(0));
    let report = |state: State, message: String| {
        protected(
            &dir.join("report.dat"),
            &Report {
                token: token.clone(),
                state,
                hwnd: hwnd.load(Ordering::SeqCst),
                pid: pid.load(Ordering::SeqCst),
                message,
                updated: now(),
            },
        )
    };
    report(State::Starting, String::new())?;
    let guard = {
        let dir = dir.clone();
        let stop = stop.clone();
        let finished = finished.clone();
        let hwnd = hwnd.clone();
        let pid = pid.clone();
        std::thread::spawn(move || {
            while !finished.load(Ordering::SeqCst) {
                if !crate::watchdog::alive(request.parent_pid, &request.parent_exe)
                    || !lease_valid(&dir, &request)
                {
                    stop.store(true, Ordering::SeqCst);
                    let started = Instant::now();
                    while !finished.load(Ordering::SeqCst)
                        && started.elapsed() < Duration::from_secs(5)
                    {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    if !finished.load(Ordering::SeqCst) {
                        close_window(
                            hwnd.load(Ordering::SeqCst),
                            pid.load(Ordering::SeqCst),
                            &request.token,
                        );
                        cleanup(&dir);
                        std::process::exit(1);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    let result = (|| {
        let _apartment = Apartment::new()?;
        let file = dir.join(format!(
            "content-{token}.{}",
            load::<Request>(&dir.join("request.dat"))?.extension
        ));
        validate(&file)?;
        let show = Show::open(&file, &token, &stop)?;
        hwnd.store(show.hwnd, Ordering::SeqCst);
        pid.store(show.pid, Ordering::SeqCst);
        let mut unresponsive: Option<Instant> = None;
        while !stop.load(Ordering::SeqCst) {
            Apartment::pump();
            if unsafe { IsWindow(show.hwnd) } == 0 {
                let alive = unsafe {
                    let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, show.pid);
                    if handle == 0 {
                        false
                    } else {
                        let alive = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
                        CloseHandle(handle);
                        alive
                    }
                };
                ensure!(alive, "PowerPoint 进程异常退出");
                report(State::Ended, String::new())?;
                break;
            }
            ensure!(
                owned_window(show.hwnd, show.pid, &token),
                "本次 PowerPoint 放映窗口已改变"
            );
            let mut reply = 0;
            let responding = unsafe {
                SendMessageTimeoutW(
                    show.hwnd,
                    WM_NULL,
                    0,
                    0,
                    SMTO_ABORTIFHUNG | SMTO_BLOCK,
                    100,
                    &mut reply,
                )
            } != 0;
            if responding {
                unresponsive = None;
            } else {
                let started = unresponsive.get_or_insert_with(Instant::now);
                ensure!(
                    started.elapsed() < Duration::from_secs(10),
                    "PowerPoint 放映窗口未响应"
                );
            }
            report(State::Showing, String::new())?;
            std::thread::sleep(Duration::from_millis(250));
        }
        drop(show);
        report(State::Ended, String::new())?;
        Ok(())
    })();
    if let Err(e) = &result {
        close_window(
            hwnd.load(Ordering::SeqCst),
            pid.load(Ordering::SeqCst),
            &token,
        );
        let _ = report(State::Fault, format!("{e:#}"));
    }
    finished.store(true, Ordering::SeqCst);
    let _ = guard.join();
    // Keep only protected status until the controller reads it; erase the plaintext now.
    for ext in ["ppt", "pptx", "pps", "ppsx"] {
        let p = dir.join(format!("content-{token}.{ext}"));
        if let Ok(meta) = fs::metadata(&p) {
            let mut permissions = meta.permissions();
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&p, permissions);
            let _ = fs::remove_file(p);
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ppt_original_bytes_require_lic_and_survive_encrypted_roundtrip() -> Result<()> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("ppt-license-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let source = dir.join("animated.pptx");
        let mut zip = zip::ZipWriter::new(fs::File::create(&source)?);
        for (name, bytes) in [
            ("[Content_Types].xml", b"<Types/>".as_slice()),
            (
                "ppt/presentation.xml",
                b"<presentation><slide/></presentation>".as_slice(),
            ),
            (
                "ppt/slides/slide1.xml",
                b"<slide><transition/><timing><animation/></timing></slide>".as_slice(),
            ),
            (
                "ppt/media/audio.wav",
                b"RIFF native media fixture".as_slice(),
            ),
        ] {
            zip.start_file(name, zip::write::FileOptions::default())?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
        validate(&source)?;
        let original = fs::read(&source)?;
        let issuer = Issuer::create()?;
        let public = issuer.spki()?;
        let key = random::<32>();
        let info = PackageInfo {
            version: 2,
            content_id: uuid::Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&public),
            issuer_public_key: b64(&public),
            display_name: "animated.pptx".into(),
            kind: "presentation".into(),
            plain_length: original.len() as u64,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: crate::hardware::STABLE.into(),
            enrollment_public_key: issuer.enrollment_public()?,
        };
        let pkg = dir.join("content.pkg");
        let exe = dir.join("content.exe");
        write_package(&source, &pkg, &info, &key)?;
        build_exe(&std::env::current_exe()?, &pkg, &exe)?;
        let auth = Arc::new(std::sync::Mutex::new(crate::session::Session::new(
            info.clone(),
        )));
        let rejected = Presentation::open(
            Reader::new(Package::open_exe(&exe)?, key.to_vec())?,
            Some(auth.clone()),
            &info,
        )
        .err()
        .context("未授权不得打开 PPT")?;
        assert!(rejected.to_string().contains("授权"));
        let device = platform::Device::for_info(&info)?;
        let wire = crate::hardware::export(&info, &device)?;
        let enrollment = crate::hardware::decrypt(&issuer, &info, &wire)?;
        let catalog = Catalog {
            info: info.clone(),
            key_base64: b64(&key),
        };
        let license = issue(&issuer, &catalog, &enrollment.device, 1, now())?;
        let lic = dir.join("valid.lic");
        atomic(&lic, &encode(&license)?)?;
        let activated = auth.lock().unwrap().import(&lic, &device, &exe)?;
        let mut reader = Reader::new(Package::open_exe(&exe)?, activated)?;
        assert_eq!(reader.package.info.kind, "presentation");
        assert_eq!(&*reader.all(1024 * 1024)?, &original);
        auth.lock().unwrap().active = false;
        assert!(Presentation::open(
            Reader::new(Package::open_exe(&exe)?, key.to_vec())?,
            Some(auth),
            &info
        )
        .is_err());
        fs::remove_file(platform::state_path(info.content_id))?;
        ensure!(
            dir.parent() == Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("work").as_path()),
            "测试目录越界"
        );
        fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn closing_show_does_not_touch_unrelated_or_reused_windows() -> Result<()> {
        unsafe {
            let create = || {
                CreateWindowExW(
                    0,
                    platform::wide("STATIC").as_ptr(),
                    platform::wide("presentation guard test").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            let own = create();
            let other = create();
            ensure!(own != 0 && other != 0, "测试窗口创建失败");
            let token = uuid::Uuid::new_v4().simple().to_string();
            let pid = std::process::id();
            SetPropW(own, property(&token).as_ptr(), 1);
            close_window(other, pid, &token);
            close_window(own, pid + 1, &token);
            Apartment::pump();
            assert_ne!(IsWindow(own), 0);
            assert_ne!(IsWindow(other), 0);
            close_window(own, pid, &token);
            Apartment::pump();
            assert_eq!(IsWindow(own), 0);
            assert_ne!(IsWindow(other), 0);
            DestroyWindow(other);
        }
        Ok(())
    }
    #[test]
    fn presentation_format_validation_rejects_pdf_and_macro_packages() -> Result<()> {
        fs::create_dir_all(root())?;
        let dir = root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&dir)?;
        let good = dir.join("test.pptx");
        let file = fs::File::create(&good)?;
        let mut zip = zip::ZipWriter::new(file);
        for name in ["[Content_Types].xml", "ppt/presentation.xml"] {
            zip.start_file(name, zip::write::FileOptions::default())?;
            zip.write_all(b"<xml/>")?;
        }
        zip.finish()?;
        validate(&good)?;
        fs::write(dir.join("fake.ppt"), b"%PDF-1.7 fake")?;
        assert!(validate(&dir.join("fake.ppt")).is_err());
        let bad = dir.join("macro.pptx");
        let mut zip = zip::ZipWriter::new(fs::File::create(&bad)?);
        for name in [
            "[Content_Types].xml",
            "ppt/presentation.xml",
            "ppt/vbaProject.bin",
        ] {
            zip.start_file(name, zip::write::FileOptions::default())?;
            zip.write_all(b"x")?;
        }
        zip.finish()?;
        assert!(validate(&bad).is_err());
        assert!(!supported("pptm"));
        cleanup(&dir);
        Ok(())
    }
    #[test]
    fn presentation_lease_is_protected_scoped_and_bounded() -> Result<()> {
        fs::create_dir_all(root())?;
        let dir = root().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&dir)?;
        let request = Request {
            token: "1234567890abcdef1234567890abcdef".into(),
            parent_pid: std::process::id(),
            parent_exe: std::env::current_exe()?,
            extension: "pptx".into(),
        };
        let mut lease = Lease {
            token: request.token.clone(),
            sequence: 1,
            until: now() + 8,
            stop: false,
        };
        protected(&dir.join("lease.dat"), &lease)?;
        assert!(lease_valid(&dir, &request));
        lease.stop = true;
        protected(&dir.join("lease.dat"), &lease)?;
        assert!(!lease_valid(&dir, &request));
        lease.stop = false;
        lease.until = now() + 3600;
        protected(&dir.join("lease.dat"), &lease)?;
        assert!(!lease_valid(&dir, &request));
        lease.until = now() - 1;
        protected(&dir.join("lease.dat"), &lease)?;
        assert!(!lease_valid(&dir, &request));
        assert!(checked_dir(&platform::root()).is_err());
        cleanup(&dir);
        Ok(())
    }
}
