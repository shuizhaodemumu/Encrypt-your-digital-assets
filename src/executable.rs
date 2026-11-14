//! Authorized single-file EXE launch using normal Windows process creation.
//! A Job Object bounds process lifetime; it never restarts anything or registers startup.
use crate::{core::*, platform, session::Authorization};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};
pub fn validate(path: &Path) -> Result<()> {
    let mut f = fs::File::open(path)?;
    let len = f.metadata()?.len();
    ensure!(len >= 64, "EXE 文件过短");
    let mut dos = [0u8; 64];
    f.read_exact(&mut dos)?;
    ensure!(&dos[..2] == b"MZ", "不是有效 Windows EXE");
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    ensure!(offset >= 64 && offset + 26 <= len, "EXE PE 头无效");
    f.seek(SeekFrom::Start(offset))?;
    let mut pe = [0u8; 26];
    f.read_exact(&mut pe)?;
    ensure!(&pe[..4] == b"PE\0\0", "EXE PE 签名无效");
    let flags = u16::from_le_bytes(pe[22..24].try_into().unwrap());
    ensure!(
        flags & 2 != 0 && flags & 0x2000 == 0,
        "请选择可执行程序，而不是 DLL"
    );
    let magic = u16::from_le_bytes(pe[24..26].try_into().unwrap());
    ensure!(matches!(magic, 0x10b | 0x20b), "不支持此 EXE 格式");
    ensure!(
        Package::open_exe(path).is_err(),
        "请选择原始 EXE，不要再次加密已打包的授权程序"
    );
    Ok(())
}
fn root() -> PathBuf {
    platform::root().join("executables")
}
fn owned(dir: &Path) -> bool {
    dir.parent() == Some(root().as_path())
        && dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| uuid::Uuid::parse_str(n).is_ok())
        && fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn cleanup(dir: &Path) {
    if !owned(dir) {
        return;
    }
    crate::application_bundle::remove_tree(dir);
}
pub fn cleanup_abandoned() {
    if let Ok(items) = fs::read_dir(root()) {
        for item in items.flatten() {
            let dir = item.path();
            if !owned(&dir) {
                continue;
            }
            if let Ok(lock) = fs::OpenOptions::new()
                .write(true)
                .share_mode(0)
                .open(dir.join("session.lock"))
            {
                drop(lock);
                cleanup(&dir);
            }
        }
    }
}
struct Handle(isize);
impl Drop for Handle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
struct Running {
    job: Handle,
    process: Handle,
    dir: PathBuf,
    lock: Option<fs::File>,
}
impl Running {
    fn active(&self) -> Result<bool> {
        unsafe {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            ensure!(
                QueryInformationJobObject(
                    self.job.0,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    std::ptr::null_mut()
                ) != 0,
                "读取程序状态失败：{}",
                std::io::Error::last_os_error()
            );
            Ok(accounting.ActiveProcesses != 0)
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.job.0, 1);
            WaitForSingleObject(self.process.0, 2000);
        }
        self.lock.take();
        cleanup(&self.dir);
    }
}
fn permitted(auth: &Authorization, cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::SeqCst), "程序启动已取消");
    auth.lock()
        .map_err(|_| anyhow::anyhow!("授权状态不可用"))?
        .validate()
}
fn launch(
    mut reader: Reader,
    auth: Authorization,
    info: PackageInfo,
    working: PathBuf,
    cancel: Arc<AtomicBool>,
) -> Result<Running> {
    permitted(&auth, &cancel)?;
    fs::create_dir_all(root())?;
    let dir = root().join(uuid::Uuid::new_v4().to_string());
    fs::create_dir(&dir)?;
    let result = (|| {
        let lock = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(dir.join("session.lock"))?;
        let name = Path::new(&info.display_name)
            .file_name()
            .context("程序文件名无效")?;
        ensure!(
            Path::new(name) == Path::new(&info.display_name),
            "程序文件名不能包含路径"
        );
        let runtime = dir.join("runtime");
        fs::create_dir(&runtime)?;
        let executable = runtime.join(name);
        let file = if info.kind == "application" {
            dir.join("application.zip")
        } else {
            executable.clone()
        };
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)?;
        let mut buffer = zeroize::Zeroizing::new(vec![0u8; CHUNK]);
        loop {
            permitted(&auth, &cancel)?;
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            out.write_all(&buffer[..n])?;
        }
        out.sync_all()?;
        drop(out);
        if info.kind == "application" {
            crate::application_bundle::extract(&file, &runtime, || permitted(&auth, &cancel))?;
            fs::remove_file(&file)?;
        }
        let file = executable;
        validate(&file)?;
        permitted(&auth, &cancel)?;
        unsafe {
            let job = Handle(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
            ensure!(job.0 != 0, "无法建立程序会话");
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32
                ) != 0,
                "无法设置程序会话"
            );
            let app = platform::wide(&file.to_string_lossy());
            let mut command = platform::wide(&format!("\"{}\"", file.display()));
            let cwd = platform::wide(&runtime.to_string_lossy());
            let _ = working;
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of_val(&startup) as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            ensure!(CreateProcessW(app.as_ptr(),command.as_mut_ptr(),std::ptr::null(),std::ptr::null(),0,CREATE_SUSPENDED,std::ptr::null(),cwd.as_ptr(),&startup,&mut pi)!=0,"无法启动 EXE：{}。请确认 DLL 和运行环境完整；需要管理员权限的程序请先以管理员身份打开授权程序。",std::io::Error::last_os_error());
            let process = Handle(pi.hProcess);
            let thread = Handle(pi.hThread);
            if AssignProcessToJobObject(job.0, process.0) == 0 {
                TerminateProcess(process.0, 1);
                WaitForSingleObject(process.0, 2000);
                anyhow::bail!(
                    "无法关联受授权程序会话：{}",
                    std::io::Error::last_os_error()
                );
            }
            let running = Running {
                job,
                process,
                dir: dir.clone(),
                lock: Some(lock),
            };
            permitted(&auth, &cancel)?;
            ensure!(ResumeThread(thread.0) != u32::MAX, "启动程序线程失败");
            Ok(running)
        }
    })();
    if result.is_err() {
        cleanup(&dir);
    }
    result
}
pub struct Program {
    receiver: Receiver<Result<Running>>,
    running: Option<Running>,
    cancel: Arc<AtomicBool>,
    waiting: bool,
}
#[derive(PartialEq)]
pub enum State {
    Starting,
    Running,
    Ended,
}
impl Program {
    pub fn open(
        reader: Reader,
        auth: Option<Authorization>,
        info: &PackageInfo,
        working: PathBuf,
    ) -> Result<Self> {
        let auth = auth.context("EXE 必须先通过 LIC 验证")?;
        auth.lock()
            .map_err(|_| anyhow::anyhow!("授权状态不可用"))?
            .validate()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let info = info.clone();
        let (tx, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = launch(reader, auth, info, working, flag);
            let _ = tx.send(result);
        });
        Ok(Self {
            receiver,
            running: None,
            cancel,
            waiting: true,
        })
    }
    pub fn poll(&mut self) -> Result<State> {
        if self.waiting {
            match self.receiver.try_recv() {
                Ok(result) => {
                    self.waiting = false;
                    self.running = Some(result?);
                }
                Err(TryRecvError::Empty) => return Ok(State::Starting),
                Err(TryRecvError::Disconnected) => anyhow::bail!("程序启动任务异常结束"),
            }
        }
        if let Some(r) = &self.running {
            if r.active()? {
                return Ok(State::Running);
            }
        }
        self.running.take();
        Ok(State::Ended)
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.running.take();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_real_pe_and_rejects_disguised_input() -> Result<()> {
        validate(&std::env::current_exe()?)?;
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("invalid-exe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let p = dir.join("fake.exe");
        fs::write(&p, b"not an executable")?;
        assert!(validate(&p).is_err());
        fs::remove_dir_all(dir)?;
        Ok(())
    }
}
