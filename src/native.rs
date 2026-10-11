use crate::{
    core::*,
    platform,
    session::{authorized, Authorization},
};
use anyhow::{ensure, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CString},
    fs,
    path::PathBuf,
    ptr,
    sync::Mutex,
};

pub fn runtime() -> Result<PathBuf> {
    static ZIP: &[u8] = include_bytes!("../assets/runtime.zip");
    let root = platform::root()
        .join("rust-runtime")
        .join(&fingerprint(ZIP)[..20]);
    let ready = root.join("complete.txt");
    if ready.exists() {
        return Ok(root);
    }
    fs::create_dir_all(&root)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(ZIP))?;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i)?;
        let relative = e
            .enclosed_name()
            .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("运行库包含非法路径")))?;
        let path = root.join(relative);
        if e.is_dir() {
            fs::create_dir_all(path)?;
        } else {
            if let Some(d) = path.parent() {
                fs::create_dir_all(d)?;
            }
            let tmp = path.with_extension("extracting");
            let mut f = fs::File::create(&tmp)?;
            std::io::copy(&mut e, &mut f)?;
            f.sync_all()?;
            drop(f);
            platform::replace_file(&tmp, &path)?;
        }
    }
    atomic(&ready, b"complete")?;
    Ok(root)
}
struct Input {
    reader: Mutex<Reader>,
    auth: Option<Authorization>,
}
unsafe extern "C" fn open(opaque: *mut c_void, data: *mut *mut c_void, size: *mut u64) -> i32 {
    let i = &*(opaque as *mut Input);
    if authorized(&i.auth).is_err() {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => {
            r.position = 0;
            *size = r.package.info.plain_length;
            *data = opaque;
            0
        }
        Err(_) => -1,
    }
}
unsafe extern "C" fn read(opaque: *mut c_void, buffer: *mut c_void, size: usize) -> isize {
    let i = &*(opaque as *mut Input);
    if authorized(&i.auth).is_err() {
        return -1;
    }
    if size > 16 * 1024 * 1024 {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => r
            .read(std::slice::from_raw_parts_mut(buffer as *mut u8, size))
            .map(|n| n as isize)
            .unwrap_or(-1),
        Err(_) => -1,
    }
}
unsafe extern "C" fn seek(opaque: *mut c_void, pos: u64) -> i32 {
    let i = &*(opaque as *mut Input);
    if authorized(&i.auth).is_err() {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => {
            if r.seek(pos).is_ok() {
                0
            } else {
                -1
            }
        }
        Err(_) => -1,
    }
}
unsafe extern "C" fn close(_: *mut c_void) {}
type New = unsafe extern "C" fn(i32, *const *const c_char) -> *mut c_void;
type MediaCallbacks = unsafe extern "C" fn(
    *mut c_void,
    Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_void, *mut u64) -> i32>,
    Option<unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> isize>,
    Option<unsafe extern "C" fn(*mut c_void, u64) -> i32>,
    Option<unsafe extern "C" fn(*mut c_void)>,
    *mut c_void,
) -> *mut c_void;
type Release = unsafe extern "C" fn(*mut c_void);
pub struct Video {
    _lib: Library,
    _core: Library,
    instance: *mut c_void,
    media: *mut c_void,
    player: *mut c_void,
    input: Box<Input>,
    stop: Release,
    release_player: Release,
    release_media: Release,
    release_instance: Release,
    play: unsafe extern "C" fn(*mut c_void) -> i32,
    state: unsafe extern "C" fn(*mut c_void) -> i32,
    tracks: unsafe extern "C" fn(*mut c_void) -> i32,
    time: unsafe extern "C" fn(*mut c_void) -> i64,
    pause: unsafe extern "C" fn(*mut c_void),
    set_pos: unsafe extern "C" fn(*mut c_void, f32),
    volume: unsafe extern "C" fn(*mut c_void, i32) -> i32,
}
impl Video {
    pub fn open(
        reader: Reader,
        auth: Option<Authorization>,
        hwnd: isize,
        repeat: bool,
    ) -> Result<Self> {
        let root = runtime()?;
        unsafe {
            let plugins = CString::new(root.join("plugins").to_string_lossy().as_bytes())?;
            std::env::set_var("VLC_PLUGIN_PATH", plugins.to_str()?);
            let core = Library::new(root.join("libvlccore.dll"))?;
            let lib = Library::new(root.join("libvlc.dll"))?;
            let new: New = *lib.get(b"libvlc_new\0")?;
            let argv = [
                CString::new("--no-video-title-show")?,
                CString::new("--no-plugins-cache")?,
            ];
            let args: Vec<_> = argv.iter().map(|s| s.as_ptr()).collect();
            let instance = new(args.len() as i32, args.as_ptr());
            ensure!(!instance.is_null(), crate::i18n::text("播放器初始化失败"));
            let mut input = Box::new(Input {
                reader: Mutex::new(reader),
                auth,
            });
            let media_new: MediaCallbacks = *lib.get(b"libvlc_media_new_callbacks\0")?;
            let media = media_new(
                instance,
                Some(open),
                Some(read),
                Some(seek),
                Some(close),
                (&mut *input as *mut Input).cast(),
            );
            ensure!(
                !media.is_null(),
                crate::i18n::text("播放器读取接口初始化失败")
            );
            if repeat {
                // VLC 3 rejects negative input-repeat values. Repeat inside its
                // input pipeline instead of destroying the vout at each EOF.
                // A fallback below handles the practically unreachable limit.
                let add_option: unsafe extern "C" fn(*mut c_void, *const c_char) =
                    *lib.get(b"libvlc_media_add_option\0")?;
                let option = CString::new(":input-repeat=2147483647")?;
                add_option(media, option.as_ptr());
            }
            let player_new: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                *lib.get(b"libvlc_media_player_new_from_media\0")?;
            let player = player_new(media);
            ensure!(!player.is_null(), crate::i18n::text("播放器创建失败"));
            let set_hwnd: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                *lib.get(b"libvlc_media_player_set_hwnd\0")?;
            set_hwnd(player, hwnd as *mut c_void);
            let keyboard: unsafe extern "C" fn(*mut c_void, u32) =
                *lib.get(b"libvlc_video_set_key_input\0")?;
            let mouse: unsafe extern "C" fn(*mut c_void, u32) =
                *lib.get(b"libvlc_video_set_mouse_input\0")?;
            keyboard(player, 0);
            mouse(player, 0);
            let v = Self {
                instance,
                media,
                player,
                input,
                stop: *lib.get(b"libvlc_media_player_stop\0")?,
                release_player: *lib.get(b"libvlc_media_player_release\0")?,
                release_media: *lib.get(b"libvlc_media_release\0")?,
                release_instance: *lib.get(b"libvlc_release\0")?,
                play: *lib.get(b"libvlc_media_player_play\0")?,
                state: *lib.get(b"libvlc_media_player_get_state\0")?,
                tracks: *lib.get(b"libvlc_video_get_track_count\0")?,
                time: *lib.get(b"libvlc_media_player_get_time\0")?,
                pause: *lib.get(b"libvlc_media_player_pause\0")?,
                set_pos: *lib.get(b"libvlc_media_player_set_position\0")?,
                volume: *lib.get(b"libvlc_audio_set_volume\0")?,
                _lib: lib,
                _core: core,
            };
            ensure!((v.play)(v.player) == 0, crate::i18n::text("媒体播放失败"));
            Ok(v)
        }
    }
    pub fn state(&self) -> i32 {
        unsafe { (self.state)(self.player) }
    }
    pub fn has_video(&self) -> bool {
        unsafe { (self.tracks)(self.player) > 0 }
    }
    pub fn time(&self) -> i64 {
        unsafe { (self.time)(self.player) }
    }
    pub fn play(&self) -> Result<()> {
        authorized(&self.input.auth)?;
        unsafe { ensure!((self.play)(self.player) == 0, crate::i18n::text("播放失败")) };
        Ok(())
    }
    pub fn pause(&self) {
        unsafe { (self.pause)(self.player) }
    }
    pub fn seek(&self, p: f32) -> Result<()> {
        authorized(&self.input.auth)?;
        unsafe { (self.set_pos)(self.player, p) };
        Ok(())
    }
    pub fn volume(&self, n: i32) {
        unsafe {
            (self.volume)(self.player, n);
        }
    }
    pub fn repeat(&self) -> Result<()> {
        authorized(&self.input.auth)?;
        unsafe { (self.stop)(self.player) };
        self.input.reader.lock().unwrap().seek(0)?;
        self.play()
    }
}
impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            (self.stop)(self.player);
            (self.release_player)(self.player);
            (self.release_media)(self.media);
            (self.release_instance)(self.instance);
        }
    }
}

#[repr(C)]
struct FileAccess {
    length: u32,
    callback: Option<unsafe extern "C" fn(*mut c_void, u32, *mut u8, u32) -> i32>,
    param: *mut c_void,
}
unsafe extern "C" fn pdf_read(param: *mut c_void, pos: u32, buf: *mut u8, size: u32) -> i32 {
    let input = &*(param as *const Input);
    if size > 16 * 1024 * 1024 || authorized(&input.auth).is_err() {
        return 0;
    }
    match input.reader.lock() {
        Ok(mut r) => {
            if r.seek(pos as u64).is_err() {
                return 0;
            }
            r.read(std::slice::from_raw_parts_mut(buf, size as usize))
                .map(|n| if n == size as usize { 1 } else { 0 })
                .unwrap_or(0)
        }
        Err(_) => 0,
    }
}
pub struct Pdf {
    _lib: Library,
    input: Box<Input>,
    _access: Box<FileAccess>,
    document: *mut c_void,
    pub pages: i32,
    load_page: unsafe extern "C" fn(*mut c_void, i32) -> *mut c_void,
    close_page: Release,
    close_doc: Release,
    width: unsafe extern "C" fn(*mut c_void) -> f64,
    height: unsafe extern "C" fn(*mut c_void) -> f64,
    bitmap: unsafe extern "C" fn(i32, i32, i32, *mut c_void, i32) -> *mut c_void,
    fill: unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, u32),
    render: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, i32, i32, i32, i32, i32),
    destroy: Release,
}
pub struct Bitmap {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}
impl Pdf {
    pub fn open(reader: Reader, auth: Option<Authorization>) -> Result<Self> {
        ensure!(
            reader.package.info.plain_length <= u32::MAX as u64,
            crate::i18n::text("PDF 超过 4 GiB")
        );
        let len = reader.package.info.plain_length as u32;
        unsafe {
            let lib = Library::new(runtime()?.join("pdfium.dll"))?;
            let init: unsafe extern "C" fn() = *lib.get(b"FPDF_InitLibrary\0")?;
            init();
            let mut input = Box::new(Input {
                reader: Mutex::new(reader),
                auth,
            });
            let mut access = Box::new(FileAccess {
                length: len,
                callback: Some(pdf_read),
                param: (&mut *input as *mut Input).cast(),
            });
            let load: unsafe extern "C" fn(*mut FileAccess, *const c_char) -> *mut c_void =
                *lib.get(b"FPDF_LoadCustomDocument\0")?;
            let document = load(&mut *access, ptr::null());
            ensure!(
                !document.is_null(),
                crate::i18n::text("PDF 无法打开，可能损坏或带密码")
            );
            let count: unsafe extern "C" fn(*mut c_void) -> i32 =
                *lib.get(b"FPDF_GetPageCount\0")?;
            let pages = count(document);
            ensure!(pages > 0, crate::i18n::text("PDF 没有页面"));
            Ok(Self {
                document,
                pages,
                input,
                _access: access,
                load_page: *lib.get(b"FPDF_LoadPage\0")?,
                close_page: *lib.get(b"FPDF_ClosePage\0")?,
                close_doc: *lib.get(b"FPDF_CloseDocument\0")?,
                width: *lib.get(b"FPDF_GetPageWidth\0")?,
                height: *lib.get(b"FPDF_GetPageHeight\0")?,
                bitmap: *lib.get(b"FPDFBitmap_CreateEx\0")?,
                fill: *lib.get(b"FPDFBitmap_FillRect\0")?,
                render: *lib.get(b"FPDF_RenderPageBitmap\0")?,
                destroy: *lib.get(b"FPDFBitmap_Destroy\0")?,
                _lib: lib,
            })
        }
    }
    pub fn page(&self, n: i32) -> Result<Bitmap> {
        authorized(&self.input.auth)?;
        ensure!(n >= 0 && n < self.pages, crate::i18n::text("PDF 页码无效"));
        unsafe {
            let page = (self.load_page)(self.document, n);
            ensure!(!page.is_null(), crate::i18n::text("PDF 页面无法读取"));
            let result = (|| {
                let w = (self.width)(page);
                let h = (self.height)(page);
                ensure!(
                    w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite(),
                    crate::i18n::text("PDF 页面尺寸无效")
                );
                let scale = 2f64.min(2000.0 / w).min(2500.0 / h);
                let width = ((w * scale) as i32).max(1);
                let height = ((h * scale) as i32).max(1);
                let mut bgra = vec![255; width as usize * height as usize * 4];
                let bitmap = (self.bitmap)(width, height, 4, bgra.as_mut_ptr().cast(), width * 4);
                ensure!(!bitmap.is_null(), crate::i18n::text("PDF 渲染内存分配失败"));
                (self.fill)(bitmap, 0, 0, width, height, 0xffffffff);
                (self.render)(bitmap, page, 0, 0, width, height, 0, 1);
                (self.destroy)(bitmap);
                authorized(&self.input.auth)?;
                Ok(Bitmap {
                    width,
                    height,
                    bgra,
                })
            })();
            (self.close_page)(page);
            result
        }
    }
}
impl Drop for Pdf {
    fn drop(&mut self) {
        unsafe { (self.close_doc)(self.document) }
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        self.bgra.fill(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a short MP4 fixture and Windows desktop"]
    fn encrypted_video_repeats_without_rebuilding_output() -> Result<()> {
        use std::{
            collections::BTreeSet,
            sync::Arc,
            time::{Duration, Instant},
        };
        use windows_sys::Win32::{
            Graphics::Gdi::{GetDC, GetPixel, ReleaseDC},
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::*,
        };
        unsafe extern "system" fn collect(hwnd: isize, param: isize) -> i32 {
            let mut name = [0u16; 128];
            let len = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
            if String::from_utf16_lossy(&name[..len as usize]).contains("VLC video") {
                (*(param as *mut BTreeSet<isize>)).insert(hwnd);
            }
            1
        }
        struct TestWindow(isize);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        let fixture = PathBuf::from(
            std::env::var_os("OFFLINE_LOOP_FIXTURE")
                .ok_or_else(|| anyhow::anyhow!("Set OFFLINE_LOOP_FIXTURE to a short MP4"))?,
        );
        ensure!(fs::metadata(&fixture)?.len() > 0, "Empty fixture");
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("loop-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&folder)?;
        let issuer = Issuer::create()?;
        let public = issuer.spki()?;
        let info = PackageInfo {
            version: 1,
            content_id: uuid::Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&public),
            issuer_public_key: b64(&public),
            display_name: "loop-fixture.mp4".into(),
            kind: "media".into(),
            plain_length: fs::metadata(&fixture)?.len(),
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: String::new(),
            enrollment_public_key: String::new(),
        };
        let key = random::<32>();
        let encrypted = folder.join("encrypted.pkg");
        write_package(&fixture, &encrypted, &info, &key)?;
        let reader = Reader::new(
            Package::header(
                fs::File::open(&encrypted)?,
                0,
                fs::metadata(&encrypted)?.len(),
            )?,
            key.to_vec(),
        )?;
        // In-memory active-session fixture. Signature/enrollment validation is
        // separately covered by the normal tests; no device state is persisted.
        let body = LicenseBody {
            version: 1,
            license_id: uuid::Uuid::new_v4(),
            content_id: info.content_id,
            issuer_fingerprint: info.issuer_fingerprint.clone(),
            device_id: "1".into(),
            issued_at_utc_seconds: now(),
            activate_before_utc_seconds: now() + 60,
            days: 1,
            wrapped_content_key: b64(&[0; 384]),
        };
        let mut session = crate::session::Session::new(info);
        session.state.current = Some(activate(
            SignedLicense {
                body_base64: String::new(),
                signature_base64: String::new(),
            },
            &body,
            now(),
        )?);
        session.body = Some(body);
        session.active = true;
        let auth = Arc::new(Mutex::new(session));
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                platform::wide("STATIC").as_ptr(),
                platform::wide("Encrypted video loop regression").as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN | 0x0004,
                40,
                40,
                900,
                300,
                0,
                0,
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        ensure!(hwnd != 0, "Could not create loop test window");
        let window = TestWindow(hwnd);
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        let video = Video::open(reader, Some(auth.clone()), hwnd, true)?;
        let started = Instant::now();
        let mut previous = 0;
        let mut loops = 0;
        let mut first_output: Option<BTreeSet<isize>> = None;
        let mut missing_output_samples = 0;
        let mut white_edge_samples = 0;
        let mut ended_samples = 0;
        let mut samples = 0;
        while started.elapsed() < Duration::from_secs(45) && loops < 4 {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            let time = video.time().max(0);
            if time + 500 < previous {
                loops += 1;
            }
            previous = time;
            ensure!(video.state() != 7, "Media decoder failed");
            if video.state() == 6 {
                ended_samples += 1;
            }
            let mut output = BTreeSet::new();
            unsafe {
                EnumChildWindows(hwnd, Some(collect), &mut output as *mut _ as isize);
            }
            if let Some(first) = &first_output {
                samples += 1;
                if output.is_empty() {
                    missing_output_samples += 1;
                } else {
                    ensure!(
                        &output == first,
                        "Video output window was recreated at the loop boundary"
                    );
                }
                unsafe {
                    let dc = GetDC(hwnd);
                    if dc != 0 {
                        for (x, y) in [(2, 2), (897, 2), (2, 297), (897, 297)] {
                            let pixel = GetPixel(dc, x, y);
                            if pixel != u32::MAX
                                && pixel & 255 > 235
                                && (pixel >> 8) & 255 > 235
                                && (pixel >> 16) & 255 > 235
                            {
                                white_edge_samples += 1;
                            }
                        }
                        ReleaseDC(hwnd, dc);
                    }
                }
            } else if !output.is_empty() && time > 250 {
                first_output = Some(output);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("LOOP_METRICS loops={loops} samples={samples} output_disappearances={missing_output_samples} white_edge_samples={white_edge_samples} ended_samples={ended_samples}");
        ensure!(
            loops >= 4,
            "Four native repeat boundaries were not observed"
        );
        ensure!(
            samples > 0
                && missing_output_samples == 0
                && white_edge_samples == 0
                && ended_samples == 0,
            "Loop continuity regression"
        );
        auth.lock().unwrap().active = false;
        let mut buffer = [0u8; 16];
        let opaque = (&*video.input as *const Input as *mut Input).cast();
        assert_eq!(
            unsafe { read(opaque, buffer.as_mut_ptr().cast(), buffer.len()) },
            -1
        );
        assert_eq!(unsafe { seek(opaque, 0) }, -1);
        println!("REVOKED_AUTHORIZATION_STILL_BLOCKS_READ_AND_SEEK");
        drop(video);
        drop(window);
        fs::remove_file(encrypted)?;
        fs::remove_dir(folder)?;
        Ok(())
    }
    #[test]
    fn pdfium_decrypts_via_memory_callbacks() -> Result<()> {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("work");
        let dir = base.join(format!("pdf-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>",
            "<< /Length 0 >>\nstream\n\nendstream",
        ];
        let mut data = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, o) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
        }
        let start = data.len();
        data.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in &offsets[1..] {
            data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        data.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n").as_bytes(),
        );
        let issuer = Issuer::create()?;
        let public = issuer.spki()?;
        let info = PackageInfo {
            version: 1,
            content_id: uuid::Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&public),
            issuer_public_key: b64(&public),
            display_name: "memory.pdf".into(),
            kind: "pdf".into(),
            plain_length: data.len() as u64,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: String::new(),
            enrollment_public_key: String::new(),
        };
        let key = random::<32>();
        let input = dir.join("input.pdf");
        let encrypted = dir.join("encrypted.pkg");
        fs::write(&input, &data)?;
        write_package(&input, &encrypted, &info, &key)?;
        fs::remove_file(&input)?;
        let reader = Reader::new(
            Package::header(
                fs::File::open(&encrypted)?,
                0,
                fs::metadata(&encrypted)?.len(),
            )?,
            key.to_vec(),
        )?;
        let pdf = Pdf::open(reader, None)?;
        assert_eq!(pdf.pages, 1);
        let page = pdf.page(0)?;
        assert!(page.width > 0 && page.height > 0 && page.bgra.iter().any(|b| *b == 255));
        assert!(pdf.page(1).is_err());
        drop(page);
        drop(pdf);
        assert!(!input.exists());
        assert!(dir.starts_with(base));
        fs::remove_dir_all(dir)?;
        Ok(())
    }
}
