use crate::core::Reader;
use anyhow::{ensure, Result};
use gtk::{cairo, glib};
use std::{
    ffi::c_void,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
type P = *mut c_void;
#[link(name = "poppler-glib")]
extern "C" {
    fn poppler_document_new_from_bytes(bytes: P, password: *const i8, error: *mut P) -> P;
    fn poppler_document_get_n_pages(doc: P) -> i32;
    fn poppler_document_get_page(doc: P, index: i32) -> P;
    fn poppler_page_get_size(page: P, width: *mut f64, height: *mut f64);
    fn poppler_page_render(page: P, context: P);
}
pub struct Pdf {
    doc: P,
}
impl Pdf {
    pub fn open(reader: &mut Reader) -> Result<Self> {
        unsafe {
            let bytes = reader.all(256 * 1024 * 1024)?;
            let b = glib::ffi::g_bytes_new(bytes.as_ptr() as _, bytes.len());
            let mut error = std::ptr::null_mut();
            let doc = poppler_document_new_from_bytes(b as P, std::ptr::null(), &mut error);
            glib::ffi::g_bytes_unref(b);
            if !error.is_null() {
                glib::ffi::g_error_free(error as _);
            }
            ensure!(!doc.is_null(), "PDF 无法打开，可能损坏或另有密码");
            Ok(Self { doc })
        }
    }
    pub fn pages(&self) -> i32 {
        unsafe { poppler_document_get_n_pages(self.doc) }
    }
    pub fn draw(&self, index: i32, ctx: &cairo::Context, width: i32, height: i32) {
        unsafe {
            let page = poppler_document_get_page(self.doc, index);
            if page.is_null() {
                return;
            }
            let (mut w, mut h) = (0.0, 0.0);
            poppler_page_get_size(page, &mut w, &mut h);
            if w > 0.0 && h > 0.0 {
                let s = (width as f64 / w).min(height as f64 / h);
                let _ = ctx.save();
                ctx.translate((width as f64 - w * s) / 2.0, (height as f64 - h * s) / 2.0);
                ctx.scale(s, s);
                ctx.set_source_rgb(1.0, 1.0, 1.0);
                ctx.rectangle(0.0, 0.0, w, h);
                let _ = ctx.fill();
                poppler_page_render(page, ctx.to_raw_none() as P);
                let _ = ctx.restore();
            }
            glib::gobject_ffi::g_object_unref(page as _);
        }
    }
}
impl Drop for Pdf {
    fn drop(&mut self) {
        unsafe {
            glib::gobject_ffi::g_object_unref(self.doc as _);
        }
    }
}
pub struct Presentation {
    pub child: Child,
    _dir: tempfile::TempDir,
    started: Instant,
    pub directory: PathBuf,
}
impl Presentation {
    pub fn open(reader: &mut Reader, probe: bool) -> Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let dir = tempfile::Builder::new()
            .prefix("encrypt-assets-show-")
            .tempdir_in(runtime)?;
        let ext = std::path::Path::new(&reader.package.info.display_name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("pptx");
        ensure!(
            ["ppt", "pptx", "pps", "ppsx", "odp"].contains(&ext.to_ascii_lowercase().as_str()),
            "幻灯片扩展名无效"
        );
        let path = dir.path().join(format!("presentation.{ext}"));
        crate::core::atomic(&path, &reader.all(1024 * 1024 * 1024)?)?;
        let script = dir.path().join("host.py");
        std::fs::write(&script, include_str!("../assets/impress_host.py"))?;
        let python =
            std::env::var_os("ENCRYPT_ASSETS_PYTHON").unwrap_or_else(|| "/usr/bin/python3".into());
        let mut command = Command::new(python);
        command
            .arg(&script)
            .arg(dir.path())
            .arg(path)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if probe {
            command.arg("--probe");
        }
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = command.spawn()?;
        let directory = dir.path().to_path_buf();
        Ok(Self {
            child,
            _dir: dir,
            started: Instant::now(),
            directory,
        })
    }
    pub fn poll(&mut self) -> Result<bool> {
        if let Some(s) = self.child.try_wait()? {
            ensure!(
                s.success(),
                "LibreOffice 放映失败，请检查 Impress 与 python3-uno"
            );
            return Ok(false);
        }
        if self.started.elapsed() > Duration::from_secs(45) {
            let m = std::fs::metadata(self.directory.join("heartbeat"))?;
            ensure!(
                m.modified()?.elapsed().unwrap_or_default() < Duration::from_secs(30),
                "LibreOffice 放映失去响应"
            );
        }
        Ok(true)
    }
}
impl Drop for Presentation {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGTERM);
        }
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none() {
            if start.elapsed() > Duration::from_secs(3) {
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
                let _ = self.child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.wait();
    }
}
