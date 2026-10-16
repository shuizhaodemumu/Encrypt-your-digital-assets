use crate::{
    core::*,
    native::{Bitmap, Pdf, Video},
    platform::{self, wide, Device},
    session::{Authorization, Session},
};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::Read,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::{GetModuleHandleW, LoadLibraryW},
    UI::{Controls::Dialogs::*, Controls::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
const CLASS: &str = "RustVlcOfflineWindow";
const TBM_GETPOS: u32 = 0x0400;
const SS_BLACKRECT: u32 = 0x0004;
const GCLP_HBRBACKGROUND: i32 = -10;
const PROMPT: &str = "RustOfflinePrompt";
const ACTIVATE: usize = 100;
const LOAD: usize = 101;
const CREATE: usize = 102;
const BACKUP: usize = 103;
const IMPORT: usize = 104;
const BUILD: usize = 105;
const SELECT: usize = 106;
const ISSUE: usize = 107;
const DEVICE: usize = 108;
const PREV: usize = 109;
const NEXT: usize = 110;
const PLAY: usize = 111;
const PAUSE: usize = 112;
const INSTALL: usize = 113;
const RENEW: usize = 114;
const EXPORT: usize = 115;
const FULLSCREEN: usize = 116;
const CLOSE_PLAYER: usize = 117;
const STARTUP_REPAIR: usize = 118;
const LANGUAGE_CHINESE: usize = 201;
const LANGUAGE_ENGLISH: usize = 202;
pub enum Role {
    Administrator,
    Device,
    Viewer(PackageInfo),
    Setup,
}
pub struct App {
    pub role: Role,
    busy: bool,
    hwnd: isize,
    status: isize,
    controls: Vec<isize>,
    view_controls: Vec<isize>,
    seekbar: isize,
    volbar: isize,
    surface: isize,
    text: isize,
    days: isize,
    window_value: isize,
    window_unit: isize,
    signing_summary: isize,
    signing_options: crate::issuance::Options,
    video: Option<Video>,
    presentation: Option<crate::presentation::Presentation>,
    watch: Option<crate::watchdog::Client>,
    pdf: Option<Pdf>,
    bitmap: Option<Bitmap>,
    page: i32,
    issuer: Option<Issuer>,
    input: Option<PathBuf>,
    catalog: Option<Catalog>,
    device: Option<Device>,
    auth: Option<Authorization>,
    exe: PathBuf,
    fullscreen: bool,
    windowed: RECT,
    loop_count: u32,
    last_media_time: i64,
    loop_fallbacks: u32,
    temp: Vec<PathBuf>,
    font: isize,
    language_menu: isize,
    artwork: Bitmap,
    side_artwork: Bitmap,
    media_clock: isize,
    dock_visible: bool,
    dock_until: Instant,
    cursor: POINT,
    fullscreen_requested: bool,
    startup_notice: Option<String>,
}
fn set_text(hwnd: isize, s: &str) {
    unsafe {
        SetWindowTextW(hwnd, wide(s).as_ptr());
    }
}
fn text(hwnd: isize) -> String {
    unsafe {
        let mut b = vec![0; GetWindowTextLengthW(hwnd) as usize + 1];
        GetWindowTextW(hwnd, b.as_mut_ptr(), b.len() as i32);
        String::from_utf16_lossy(&b[..b.len() - 1])
    }
}
fn control(
    parent: isize,
    class: &str,
    label: &str,
    id: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    extra: u32,
    font: isize,
) -> isize {
    unsafe {
        let hwnd = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE | extra,
            x,
            y,
            w,
            h,
            parent,
            id as isize,
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
        hwnd
    }
}
pub fn message(s: &str) {
    unsafe {
        MessageBoxW(
            0,
            wide(s).as_ptr(),
            wide(crate::i18n::text("离线文件授权 · VLC视频重构版")).as_ptr(),
            MB_OK,
        );
    }
}
pub fn file_dialog(
    parent: isize,
    save: bool,
    title: &str,
    filter: &str,
    default: &str,
) -> Option<PathBuf> {
    unsafe {
        let mut buffer = vec![0u16; 32768];
        let name = wide(default);
        let n = name.len().min(buffer.len());
        buffer[..n].copy_from_slice(&name[..n]);
        let t = wide(title);
        let f: Vec<u16> = filter.encode_utf16().chain([0, 0]).collect();
        let mut dlg: OPENFILENAMEW = std::mem::zeroed();
        dlg.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dlg.hwndOwner = parent;
        dlg.lpstrTitle = t.as_ptr();
        dlg.lpstrFilter = f.as_ptr();
        dlg.lpstrFile = buffer.as_mut_ptr();
        dlg.nMaxFile = buffer.len() as u32;
        dlg.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            };
        let ok = if save {
            GetSaveFileNameW(&mut dlg)
        } else {
            GetOpenFileNameW(&mut dlg)
        };
        if ok == 0 {
            return None;
        }
        let n = buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..n])))
    }
}
struct Prompt {
    edit: isize,
    answer: Option<String>,
    done: bool,
}
unsafe extern "system" fn prompt_proc(h: isize, m: u32, w: usize, l: isize) -> isize {
    if m == WM_NCCREATE {
        let c = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(h, GWLP_USERDATA, c.lpCreateParams as isize);
    }
    let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut Prompt;
    if !p.is_null() {
        let p = &mut *p;
        match m {
            WM_COMMAND => {
                match w & 0xffff {
                    1 => {
                        p.answer = Some(text(p.edit));
                        p.done = true;
                        DestroyWindow(h);
                    }
                    2 => {
                        p.done = true;
                        DestroyWindow(h);
                    }
                    _ => {}
                }
                return 0;
            }
            WM_CLOSE => {
                p.done = true;
                DestroyWindow(h);
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(h, m, w, l)
}
pub fn prompt(parent: isize, title: &str, password: bool) -> Option<String> {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = wide(PROMPT);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(prompt_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: theme_brush(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let mut p = Box::new(Prompt {
            edit: 0,
            answer: None,
            done: false,
        });
        let h = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            wide(title).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            520,
            190,
            parent,
            0,
            instance,
            (&mut *p as *mut Prompt).cast(),
        );
        p.edit = control(
            h,
            "EDIT",
            "",
            10,
            20,
            25,
            470,
            30,
            WS_BORDER | ES_AUTOHSCROLL as u32 | if password { ES_PASSWORD as u32 } else { 0 },
            GetStockObject(DEFAULT_GUI_FONT),
        );
        control(
            h,
            "BUTTON",
            crate::i18n::text("确定"),
            1,
            285,
            80,
            95,
            35,
            0,
            GetStockObject(DEFAULT_GUI_FONT),
        );
        control(
            h,
            "BUTTON",
            crate::i18n::text("取消"),
            2,
            395,
            80,
            95,
            35,
            0,
            GetStockObject(DEFAULT_GUI_FONT),
        );
        EnableWindow(parent, 0);
        SetFocus(p.edit);
        let mut msg: MSG = std::mem::zeroed();
        while !p.done && GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if msg.message == WM_KEYDOWN && msg.wParam == 13 {
                SendMessageW(h, WM_COMMAND, 1, 0);
            } else if msg.message == WM_KEYDOWN && msg.wParam == 27 {
                SendMessageW(h, WM_COMMAND, 2, 0);
            } else {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        EnableWindow(parent, 1);
        SetForegroundWindow(parent);
        p.answer.take()
    }
}

impl App {
    fn fail_playback(&mut self) {
        if let Some(watch) = self.watch.as_mut() {
            watch.fault();
        }
        self.stop();
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
    fn view_button(&mut self, label: &str, id: usize, x: i32) {
        let y = if matches!(self.role, Role::Administrator) {
            110
        } else {
            62
        };
        let h = control(
            self.hwnd,
            "BUTTON",
            label,
            id,
            x,
            y,
            100,
            34,
            BS_OWNERDRAW as u32 | WS_TABSTOP,
            self.font,
        );
        self.view_controls.push(h);
    }
    fn media_controls(&mut self) {
        let y = if matches!(self.role, Role::Administrator) {
            110
        } else {
            62
        };
        self.view_button(crate::i18n::text("播放"), PLAY, 20);
        self.view_button(crate::i18n::text("暂停"), PAUSE, 130);
        unsafe {
            let init = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&init);
        }
        self.seekbar = control(
            self.hwnd,
            "msctls_trackbar32",
            "",
            120,
            245,
            y,
            320,
            34,
            TBS_HORZ,
            self.font,
        );
        self.volbar = control(
            self.hwnd,
            "msctls_trackbar32",
            crate::i18n::text("音量"),
            121,
            580,
            y,
            140,
            34,
            TBS_HORZ,
            self.font,
        );
        unsafe {
            SendMessageW(self.seekbar, TBM_SETRANGEMAX, 1, 1000);
            SendMessageW(self.volbar, TBM_SETRANGEMAX, 1, 100);
            SendMessageW(self.volbar, TBM_SETPOS, 1, 100);
        }
        self.view_controls.extend([self.seekbar, self.volbar]);
        self.media_clock = control(
            self.hwnd,
            "STATIC",
            "00:00 / 00:00",
            122,
            740,
            y,
            145,
            34,
            0,
            self.font,
        );
        self.view_controls.push(self.media_clock);
        self.view_button(crate::i18n::text("全屏"), FULLSCREEN, 890);
        self.view_button(crate::i18n::text("退出"), CLOSE_PLAYER, 1000);
    }
    pub fn run(role: Role, exe: PathBuf) -> Result<()> {
        unsafe {
            SetProcessDPIAware();
            let class = wide(CLASS);
            let instance = GetModuleHandleW(ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(0, IDC_ARROW),
                hbrBackground: theme_brush(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            LoadLibraryW(wide("Msftedit.dll").as_ptr());
            let title = match &role {
                Role::Administrator => {
                    crate::i18n::text("离线文件授权管理员 · VLC视频重构版").to_string()
                }
                Role::Device => crate::i18n::text("设备请求工具 · VLC视频重构版").into(),
                Role::Setup => crate::i18n::text("离线文件授权安装 · VLC视频重构版").into(),
                Role::Viewer(i) => i.display_name.clone(),
            };
            #[cfg(feature = "watchdog-test-mode")]
            let title = crate::tr_format!(
                "{title} [测试构建：电脑重启已禁用]",
                "{title} [TEST BUILD: system restart disabled]"
            );
            let font = CreateFontW(
                -18,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                0,
                0,
                wide("Microsoft YaHei UI").as_ptr(),
            );
            let mut app = Box::new(Self {
                role,
                busy: false,
                hwnd: 0,
                status: 0,
                controls: vec![],
                view_controls: vec![],
                seekbar: 0,
                volbar: 0,
                surface: 0,
                text: 0,
                days: 0,
                window_value: 0,
                window_unit: 0,
                signing_summary: 0,
                signing_options: crate::issuance::Options::load(),
                video: None,
                presentation: None,
                watch: None,
                pdf: None,
                bitmap: None,
                page: 0,
                issuer: None,
                input: None,
                catalog: None,
                device: None,
                auth: None,
                exe,
                fullscreen: false,
                windowed: std::mem::zeroed(),
                loop_count: 0,
                last_media_time: 0,
                loop_fallbacks: 0,
                temp: vec![],
                font,
                language_menu: 0,
                artwork: load_artwork()?,
                side_artwork: load_side_artwork()?,
                media_clock: 0,
                dock_visible: false,
                dock_until: Instant::now(),
                cursor: POINT { x: -1, y: -1 },
                fullscreen_requested: true,
                startup_notice: None,
            });
            let h = CreateWindowExW(
                0,
                class.as_ptr(),
                wide(&title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1180,
                880,
                0,
                0,
                instance,
                (&mut *app as *mut App).cast(),
            );
            ensure!(h != 0, crate::i18n::text("创建窗口失败"));
            app.hwnd = h;
            app.language_menu = CreateMenu();
            ensure!(app.language_menu != 0, "Could not create the language menu");
            AppendMenuW(
                app.language_menu,
                MF_STRING,
                LANGUAGE_CHINESE,
                wide("中文").as_ptr(),
            );
            AppendMenuW(
                app.language_menu,
                MF_STRING,
                LANGUAGE_ENGLISH,
                wide("English").as_ptr(),
            );
            SetMenu(h, app.language_menu);
            app.update_language_menu();
            app.busy = true;
            let initialized = app.initialize();
            app.busy = false;
            if let Err(e) = initialized {
                app.stop();
                DestroyWindow(h);
                DeleteObject(font);
                return Err(e);
            }
            ShowWindow(h, SW_SHOW);
            UpdateWindow(h);
            SetTimer(h, 1, 500, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                if msg.message == WM_KEYDOWN
                    && matches!(app.role, Role::Viewer(_))
                    && app.media_key(msg.wParam)?
                {
                    continue;
                }
                if msg.message == WM_KEYDOWN && msg.wParam == 9 && IsDialogMessageW(h, &msg) != 0 {
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            app.stop();
            if let Some(a) = &app.auth {
                let _ = a.lock().unwrap().save();
            }
            for path in &app.temp {
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
            DeleteObject(font);
            Ok(())
        }
    }
    fn add(&mut self, label: &str, id: usize, x: i32, y: i32, w: i32) -> isize {
        let h = control(
            self.hwnd,
            "BUTTON",
            label,
            id,
            x,
            y,
            w,
            34,
            BS_OWNERDRAW as u32 | WS_TABSTOP,
            self.font,
        );
        self.controls.push(h);
        h
    }
    fn initialize(&mut self) -> Result<()> {
        self.status = control(self.hwnd, "STATIC", "", 1, 20, 680, 1040, 50, 0, self.font);
        match &self.role {
            Role::Administrator => {
                self.add(crate::i18n::text("创建发行者密钥"), CREATE, 44, 150, 210);
                self.add(crate::i18n::text("恢复 / 解锁密钥"), LOAD, 266, 150, 210);
                self.add(crate::i18n::text("加密密钥备份"), BACKUP, 44, 198, 210);
                self.add(crate::i18n::text("导入并预览文件"), IMPORT, 266, 198, 210);
                self.add(crate::i18n::text("生成内容 EXE"), BUILD, 44, 246, 210);
                self.add(crate::i18n::text("选择已有内容包"), SELECT, 266, 246, 210);
                control(
                    self.hwnd,
                    "STATIC",
                    crate::i18n::text("授权天数"),
                    21,
                    560,
                    150,
                    110,
                    30,
                    0,
                    self.font,
                );
                self.days = control(
                    self.hwnd,
                    "EDIT",
                    &self.signing_options.days.to_string(),
                    20,
                    680,
                    146,
                    100,
                    36,
                    WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
                    self.font,
                );
                control(
                    self.hwnd,
                    "STATIC",
                    crate::i18n::text("首次导入窗口"),
                    25,
                    560,
                    198,
                    125,
                    30,
                    0,
                    self.font,
                );
                self.window_value = control(
                    self.hwnd,
                    "EDIT",
                    &self.signing_options.value.to_string(),
                    22,
                    690,
                    194,
                    110,
                    36,
                    WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
                    self.font,
                );
                self.window_unit = control(
                    self.hwnd,
                    "COMBOBOX",
                    "",
                    23,
                    810,
                    194,
                    135,
                    160,
                    CBS_DROPDOWNLIST as u32 | WS_VSCROLL | WS_TABSTOP,
                    self.font,
                );
                self.signing_summary =
                    control(self.hwnd, "STATIC", "", 24, 560, 244, 450, 44, 0, self.font);
                self.controls.extend([
                    self.days,
                    self.window_value,
                    self.window_unit,
                    self.signing_summary,
                ]);
                self.refresh_signing_labels();
                self.add(
                    crate::i18n::text("导入设备识别码并签发"),
                    ISSUE,
                    560,
                    302,
                    300,
                );
                self.add(crate::i18n::text("设备请求工具"), DEVICE, 870, 302, 165);
                self.refresh_signing_form();
                self.status(crate::i18n::text(
                    "请创建或解锁发行者密钥，随后导入文件。视频内容 EXE 默认全屏单个循环。",
                ))
            }
            Role::Device => {
                self.status(crate::i18n::text(
                    "选择收到的内容 EXE，导出加密设备信息；也可直接在内容 EXE 中获取设备识别码。",
                ));
                self.add(
                    crate::i18n::text("选择内容并导出设备信息"),
                    EXPORT,
                    40,
                    60,
                    290,
                );
            }
            Role::Setup => {
                self.status(
                    crate::i18n::text("本版独立安装。PPT 原文件由接收端 PowerPoint 放映；Word 静态转换需要管理员电脑安装 LibreOffice。"),
                );
                self.add(crate::i18n::text("安装 Rust 版"), INSTALL, 40, 60, 200);
            }
            Role::Viewer(info) => {
                let info = info.clone();
                self.auth = Some(Arc::new(Mutex::new(Session::new(info.clone()))));
                self.activation(crate::i18n::text("请选择该设备的授权文件"));
                if platform::load_state(info.content_id)?.current.is_none() {
                    return Ok(());
                }
                self.device = Some(Device::for_info(&info)?);
                let resume = self
                    .auth
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .resume(self.device.as_ref().unwrap());
                match resume {
                    Ok(Some(key)) => self.open_viewer(key)?,
                    Ok(None) => {}
                    Err(e) => {
                        crate::watchdog::disable_startup(&info)?;
                        self.auth.as_ref().unwrap().lock().unwrap().lock();
                        self.activation(&e.to_string());
                    }
                }
            }
        }
        self.layout();
        Ok(())
    }
    fn read_signing_form(&self) -> Result<crate::issuance::Options> {
        let unit = unsafe { SendMessageW(self.window_unit, CB_GETCURSEL, 0, 0) };
        let options = crate::issuance::Options::parse(
            &text(self.days),
            &text(self.window_value),
            usize::try_from(unit).unwrap_or(usize::MAX),
        )?;
        if let Some(catalog) = &self.catalog {
            ensure!(
                catalog.info.custom_activation_window || options.seconds()? == 60,
                crate::i18n::text("此内容由旧版打包，仅支持60秒。自定义窗口请重新打包原文件。")
            );
        }
        Ok(options)
    }
    fn refresh_signing_labels(&self) {
        if self.window_unit == 0 {
            return;
        }
        unsafe {
            let selected = SendMessageW(self.window_unit, CB_GETCURSEL, 0, 0);
            SendMessageW(self.window_unit, CB_RESETCONTENT, 0, 0);
            for unit in ["秒", "分钟", "小时", "天"] {
                SendMessageW(
                    self.window_unit,
                    CB_ADDSTRING,
                    0,
                    wide(crate::i18n::text(unit)).as_ptr() as isize,
                );
            }
            SendMessageW(
                self.window_unit,
                CB_SETCURSEL,
                if selected >= 0 {
                    selected as usize
                } else {
                    self.signing_options.unit
                },
                0,
            );
            set_text(GetDlgItem(self.hwnd, 25), crate::i18n::text("首次导入窗口"));
        }
    }
    fn refresh_signing_form(&mut self) {
        if !matches!(self.role, Role::Administrator) || self.window_unit == 0 {
            return;
        }
        let parsed = self.read_signing_form();
        match &parsed {
            Ok(value) => set_text(self.signing_summary, &value.summary()),
            Err(error) => set_text(self.signing_summary, &error.to_string()),
        }
        if let Ok(value) = &parsed {
            if value != &self.signing_options {
                if let Err(error) = value.save() {
                    set_text(
                        self.signing_summary,
                        &format!("{}: {error}", crate::i18n::text("设置暂未保存")),
                    );
                }
                self.signing_options = value.clone();
            }
        }
        unsafe {
            for (id, enabled) in [
                (BACKUP, self.issuer.is_some()),
                (IMPORT, self.issuer.is_some()),
                (SELECT, self.issuer.is_some()),
                (BUILD, self.issuer.is_some() && self.input.is_some()),
                (
                    ISSUE,
                    self.issuer.is_some() && self.catalog.is_some() && parsed.is_ok(),
                ),
            ] {
                let button = GetDlgItem(self.hwnd, id as i32);
                if button != 0 {
                    EnableWindow(button, enabled as i32);
                    InvalidateRect(button, ptr::null(), 1);
                }
            }
        }
    }
    fn status(&self, s: &str) {
        set_text(self.status, s);
    }
    fn update_language_menu(&self) {
        let english = crate::i18n::current() == crate::i18n::Language::English;
        unsafe {
            CheckMenuItem(
                self.language_menu,
                LANGUAGE_CHINESE as u32,
                MF_BYCOMMAND | if english { MF_UNCHECKED } else { MF_CHECKED },
            );
            CheckMenuItem(
                self.language_menu,
                LANGUAGE_ENGLISH as u32,
                MF_BYCOMMAND | if english { MF_CHECKED } else { MF_UNCHECKED },
            );
            DrawMenuBar(self.hwnd);
        }
    }
    fn change_language(&mut self, language: crate::i18n::Language) -> Result<()> {
        crate::i18n::select(language)?;
        // Only known button captions are translated. Document text, names, paths,
        // input values, license bytes and active playback are left untouched.
        for h in self.controls.iter().chain(self.view_controls.iter()) {
            let value = text(*h);
            set_text(*h, crate::i18n::caption(&value));
        }
        unsafe {
            let label = GetDlgItem(self.hwnd, 21);
            if label != 0 {
                set_text(label, crate::i18n::text("授权天数"));
            }
        }
        let title = match &self.role {
            Role::Administrator => crate::i18n::text("离线文件授权管理员 · VLC视频重构版"),
            Role::Device => crate::i18n::text("设备请求工具 · VLC视频重构版"),
            Role::Setup => crate::i18n::text("离线文件授权安装 · VLC视频重构版"),
            Role::Viewer(info) => &info.display_name,
        };
        #[cfg(feature = "watchdog-test-mode")]
        let title = crate::tr_format!(
            "{title} [测试构建：电脑重启已禁用]",
            "{title} [TEST BUILD: system restart disabled]"
        );
        set_text(self.hwnd, &title);
        self.refresh_signing_labels();
        self.refresh_signing_form();
        self.update_language_menu();
        unsafe {
            InvalidateRect(self.hwnd, ptr::null(), 1);
        }
        self.status(crate::i18n::text(
            "语言已切换，当前内容和授权状态保持不变。",
        ));
        Ok(())
    }
    fn stop(&mut self) {
        self.last_media_time = 0;
        if let Some(mut watch) = self.watch.take() {
            watch.stop();
        }
        for h in self.view_controls.drain(..) {
            unsafe {
                DestroyWindow(h);
            }
        }
        self.seekbar = 0;
        self.volbar = 0;
        self.video.take();
        self.presentation.take();
        self.pdf.take();
        self.bitmap.take();
        if self.surface != 0 {
            unsafe { DestroyWindow(self.surface) };
            self.surface = 0;
        }
        if self.text != 0 {
            unsafe { DestroyWindow(self.text) };
            self.text = 0;
        }
    }
    fn clear_buttons(&mut self) {
        for h in self.controls.drain(..) {
            unsafe { DestroyWindow(h) };
        }
    }
    fn activation(&mut self, s: &str) {
        self.stop();
        self.clear_buttons();
        if self.fullscreen {
            unsafe {
                SetClassLongPtrW(self.hwnd, GCLP_HBRBACKGROUND, (COLOR_WINDOW + 1) as isize);
                SetWindowLongPtrW(
                    self.hwnd,
                    GWL_STYLE,
                    (WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN) as isize,
                );
                SetWindowPos(
                    self.hwnd,
                    0,
                    self.windowed.left,
                    self.windowed.top,
                    self.windowed.right - self.windowed.left,
                    self.windowed.bottom - self.windowed.top,
                    SWP_FRAMECHANGED | SWP_NOZORDER,
                );
            }
            self.fullscreen = false;
            unsafe {
                SetMenu(self.hwnd, self.language_menu);
                DrawMenuBar(self.hwnd);
            }
        }
        unsafe {
            ShowWindow(self.status, SW_SHOW);
        }
        self.status(&crate::tr_format!("{s}\n先获取本机设备识别码交给管理员，再导入收到的 .lic 文件。", "{s}\nExport this device's encrypted report to the administrator, then import the received .lic file."));
        self.add(crate::i18n::text("获取本机设备识别码"), EXPORT, 40, 55, 260);
        self.add(
            crate::i18n::text("导入固态链密钥文件"),
            ACTIVATE,
            320,
            55,
            270,
        );
        self.layout();
    }
    fn fullscreen(&mut self) {
        if self.fullscreen {
            return;
        }
        self.fullscreen = true;
        unsafe {
            GetCursorPos(&mut self.cursor);
            self.dock_until = Instant::now();
            SetClassLongPtrW(self.hwnd, GCLP_HBRBACKGROUND, GetStockObject(BLACK_BRUSH));
        }
        for h in self.view_controls.iter().chain(self.controls.iter()) {
            unsafe {
                ShowWindow(*h, SW_HIDE);
            }
        }
        self.dock_visible = false;
        unsafe {
            SetMenu(self.hwnd, 0);
            GetWindowRect(self.hwnd, &mut self.windowed);
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
            let mut m: MONITORINFO = std::mem::zeroed();
            m.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut m);
            SetWindowLongPtrW(
                self.hwnd,
                GWL_STYLE,
                (WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN) as isize,
            );
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                m.rcMonitor.left,
                m.rcMonitor.top,
                m.rcMonitor.right - m.rcMonitor.left,
                m.rcMonitor.bottom - m.rcMonitor.top,
                SWP_FRAMECHANGED,
            );
            ShowWindow(self.status, SW_HIDE);
        }
        self.layout();
    }
    fn leave_fullscreen(&mut self) {
        if !self.fullscreen {
            return;
        }
        self.fullscreen = false;
        self.fullscreen_requested = false;
        self.dock_visible = true;
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWL_STYLE,
                (WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VISIBLE) as isize,
            );
            SetWindowPos(
                self.hwnd,
                0,
                self.windowed.left,
                self.windowed.top,
                self.windowed.right - self.windowed.left,
                self.windowed.bottom - self.windowed.top,
                SWP_FRAMECHANGED | SWP_NOZORDER,
            );
            SetMenu(self.hwnd, self.language_menu);
            DrawMenuBar(self.hwnd);
            ShowWindow(self.status, SW_SHOW);
            ShowWindow(self.hwnd, SW_SHOW);
            for h in self.view_controls.iter().chain(self.controls.iter()) {
                ShowWindow(*h, SW_SHOW);
            }
        }
        self.layout();
    }
    fn media_key(&mut self, key: usize) -> Result<bool> {
        if self.video.is_none() {
            return Ok(false);
        }
        match key {
            32 => {
                let v = self.video.as_ref().unwrap();
                if v.state() == 4 {
                    v.play()?;
                } else {
                    v.pause();
                }
            }
            37 => self.video.as_ref().unwrap().skip(-10)?,
            39 => self.video.as_ref().unwrap().skip(10)?,
            122 => {
                if self.fullscreen {
                    self.leave_fullscreen();
                } else {
                    self.fullscreen_requested = true;
                    self.fullscreen();
                }
            }
            27 if self.fullscreen => self.leave_fullscreen(),
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn update_dock(&mut self) {
        if !self.fullscreen || self.video.is_none() {
            return;
        }
        unsafe {
            let mut cursor: POINT = std::mem::zeroed();
            GetCursorPos(&mut cursor);
            let mut local = cursor;
            ScreenToClient(self.hwnd, &mut local);
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rect);
            let moved = cursor.x != self.cursor.x || cursor.y != self.cursor.y;
            self.cursor = cursor;
            let inside =
                local.x >= 0 && local.x < rect.right && local.y >= 0 && local.y < rect.bottom;
            // A parked pointer must not keep controls visible. Deliberate motion
            // near the bottom reveals the dock for three seconds.
            if moved && inside && local.y > rect.bottom - 100 {
                self.dock_until = Instant::now() + Duration::from_secs(3);
            }
            let focused = GetFocus();
            let editing =
                self.view_controls.contains(&focused) && (GetAsyncKeyState(1) as u16 & 0x8000 != 0);
            let visible = Instant::now() < self.dock_until || editing;
            if visible != self.dock_visible {
                self.dock_visible = visible;
                for h in &self.view_controls {
                    ShowWindow(*h, if visible { SW_SHOW } else { SW_HIDE });
                }
                self.layout();
            }
        }
    }
    fn layout(&self) {
        unsafe {
            if self.hwnd == 0 {
                return;
            }
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut r);
            let top = if matches!(self.role, Role::Administrator) {
                if self.video.is_none() && !self.view_controls.is_empty() {
                    436
                } else {
                    386
                }
            } else if self.fullscreen {
                0
            } else {
                110
            };
            // Some D3D video outputs composite above sibling GDI controls.
            // Reserve a dock strip only while controls are requested so the
            // controls remain visible and clickable on those drivers too.
            let bottom = if self.fullscreen {
                if self.dock_visible {
                    80
                } else {
                    0
                }
            } else {
                65
            };
            if matches!(self.role, Role::Administrator) {
                let half = (r.right - 72) / 2;
                let left = 44;
                let right = 68 + half;
                let button_width = (half - 52) / 2;
                for (id, column, row) in [
                    (CREATE, 0, 0),
                    (LOAD, 1, 0),
                    (BACKUP, 0, 1),
                    (IMPORT, 1, 1),
                    (BUILD, 0, 2),
                    (SELECT, 1, 2),
                ] {
                    let h = GetDlgItem(self.hwnd, id as i32);
                    if h != 0 {
                        MoveWindow(
                            h,
                            left + column * (button_width + 12),
                            150 + row * 48,
                            button_width,
                            36,
                            1,
                        );
                    }
                }
                for (id, x, y, width, height) in [
                    (21, right, 150, 170, 30),
                    (20, right + 180, 146, 95, 36),
                    (25, right, 198, 170, 36),
                    (22, right + 180, 194, 95, 36),
                    (23, right + 287, 194, 120, 160),
                    (24, right, 244, half - 40, 48),
                    (ISSUE as i32, right, 302, half - 250, 36),
                    (DEVICE as i32, right + half - 238, 302, 190, 36),
                ] {
                    let h = GetDlgItem(self.hwnd, id);
                    if h != 0 {
                        MoveWindow(h, x, y, width, height, 1);
                    }
                }
            }
            MoveWindow(self.status, 20, r.bottom - 60, (r.right - 40).max(1), 55, 1);
            if self.surface != 0 {
                MoveWindow(
                    self.surface,
                    0,
                    top,
                    r.right,
                    (r.bottom - top - bottom).max(1),
                    1,
                );
            }
            if self.text != 0 {
                MoveWindow(
                    self.text,
                    10,
                    top,
                    (r.right - 20).max(1),
                    (r.bottom - top - bottom).max(1),
                    1,
                );
            }
            if matches!(self.role, Role::Administrator) && self.video.is_none() {
                for (index, h) in self.view_controls.iter().enumerate() {
                    MoveWindow(*h, 24 + index as i32 * 150, 386, 140, 36, 1);
                }
            }
            if self.video.is_some() {
                let y = if self.fullscreen {
                    r.bottom - 64
                } else {
                    r.bottom - 118
                };
                let width = r.right;
                for h in &self.view_controls {
                    let id = GetDlgCtrlID(*h) as usize;
                    let (x, w) = match id {
                        PLAY => (16, 64),
                        PAUSE => (88, 64),
                        120 => (170, (width - 670).max(100)),
                        121 => ((width - 480).max(300), 110),
                        122 => ((width - 350).max(430), 155),
                        FULLSCREEN => (width - 180, 100),
                        CLOSE_PLAYER => (width - 70, 60),
                        _ => continue,
                    };
                    MoveWindow(*h, x, y, w, 36, 1);
                    SetWindowPos(
                        *h,
                        HWND_TOP,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                if self.surface != 0 && !self.fullscreen {
                    MoveWindow(
                        self.surface,
                        16,
                        top,
                        (r.right - 32).max(1),
                        (r.bottom - top - 130).max(1),
                        1,
                    );
                }
            }
            InvalidateRect(self.hwnd, ptr::null(), 1);
        }
    }
    fn open_reader(
        &mut self,
        mut reader: Reader,
        auth: Option<Authorization>,
        info: &PackageInfo,
    ) -> Result<()> {
        self.stop();
        match info.kind.as_str() {
            "presentation" => {
                if matches!(self.role, Role::Viewer(_)) || crate::presentation::available() {
                    self.presentation =
                        Some(crate::presentation::Presentation::open(reader, auth, info)?);
                }
                self.view_button(crate::i18n::text("开始放映"), PLAY, 650);
            }
            "media" => {
                self.surface = control(
                    self.hwnd,
                    "STATIC",
                    "",
                    0,
                    0,
                    110,
                    1000,
                    500,
                    SS_BLACKRECT,
                    self.font,
                );
                let repeat = matches!(self.role, Role::Viewer(_))
                    && crate::native::is_video_name(&info.display_name);
                self.video = Some(Video::open(reader, auth, self.surface, repeat)?);
                self.loop_count = 0;
                self.loop_fallbacks = 0;
                self.media_controls();
            }
            "pdf" => {
                let pdf = Pdf::open(reader, auth)?;
                self.bitmap = Some(pdf.page(0)?);
                self.pdf = Some(pdf);
                self.page = 0;
                self.view_button(crate::i18n::text("上一页"), PREV, 760);
                self.view_button(crate::i18n::text("下一页"), NEXT, 875);
            }
            "text" => {
                let bytes = reader.all(20 * 1024 * 1024)?;
                let s = if bytes.starts_with(&[0xff, 0xfe]) {
                    let v: Vec<u16> = bytes[2..]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&v)
                } else if bytes.starts_with(&[0xfe, 0xff]) {
                    let v: Vec<u16> = bytes[2..]
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&v)
                } else {
                    let raw = if bytes.starts_with(&[239, 187, 191]) {
                        &bytes[3..]
                    } else {
                        &bytes
                    };
                    match std::str::from_utf8(raw) {
                        Ok(s) => s.into(),
                        Err(_) => {
                            let (s, _, err) = encoding_rs::GB18030.decode(raw);
                            ensure!(!err, crate::i18n::text("文本编码无法解码"));
                            s.into_owned()
                        }
                    }
                };
                self.text = control(
                    self.hwnd,
                    "RICHEDIT50W",
                    &s,
                    0,
                    10,
                    110,
                    1000,
                    500,
                    ES_MULTILINE as u32 | ES_READONLY as u32 | WS_VSCROLL | WS_BORDER,
                    self.font,
                );
            }
            "image" => {
                let bytes = reader.all(64 * 1024 * 1024)?;
                let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
                    .with_guessed_format()?
                    .into_dimensions()?;
                ensure!(
                    u64::from(dimensions.0) * u64::from(dimensions.1) <= 100000000,
                    crate::i18n::text("图片超过 100 MP")
                );
                let img = image::load_from_memory(&bytes)?;
                ensure!(
                    u64::from(img.width()) * u64::from(img.height()) <= 100000000,
                    crate::i18n::text("图片超过 100 MP")
                );
                let mut rgba = img.to_rgba8().into_raw();
                for p in rgba.chunks_exact_mut(4) {
                    p.swap(0, 2);
                }
                self.bitmap = Some(Bitmap {
                    width: img.width() as i32,
                    height: img.height() as i32,
                    bgra: rgba,
                });
            }
            _ => anyhow::bail!(crate::i18n::text("暂不支持此格式")),
        }
        self.layout();
        Ok(())
    }
    fn open_viewer(&mut self, key: Vec<u8>) -> Result<()> {
        let package = Package::open_exe(&self.exe)?;
        let info = package.info.clone();
        if info.kind == "presentation" && !crate::presentation::available() {
            if let Some(a) = &self.auth {
                a.lock().unwrap().active = false;
            }
            crate::watchdog::disable_startup(&info)?;
            self.activation(crate::i18n::text("此 PPT 需要安装并激活 Microsoft PowerPoint 桌面版，然后重新打开本程序；原授权仍保留。"));
            return Ok(());
        }
        self.clear_buttons();
        self.add(crate::i18n::text("安装到本机"), INSTALL, 20, 20, 160);
        self.add(crate::i18n::text("导入新授权"), RENEW, 190, 20, 160);
        self.add(
            crate::i18n::text("修复自启动"),
            STARTUP_REPAIR,
            360,
            20,
            150,
        );
        self.open_reader(Reader::new(package, key)?, self.auth.clone(), &info)?;
        if info.version == 2 {
            self.startup_notice = crate::watchdog::enable_startup(&self.exe, &info)
                .err()
                .map(|e| format!("自动启动未设置：{e}。可点击“修复自启动”重试。"));
            self.watch = Some(crate::watchdog::Client::start(&self.exe, &info)?);
        }
        self.status(if info.kind == "presentation" {crate::i18n::text("内容已激活，正在启动 PowerPoint 全屏放映；仅放映、不提供编辑和保存，按 Esc 结束本次放映。")} else {crate::i18n::text("内容已激活，运行监测与登录自动启动已启用。视频自动全屏循环，授权到期停止。")});
        if let Some(notice) = &self.startup_notice {
            self.status(notice);
        }
        Ok(())
    }
    fn prepare(&self, path: &Path) -> Result<(PathBuf, Catalog)> {
        let issuer = self
            .issuer
            .as_ref()
            .context(crate::i18n::text("请先创建或解锁密钥"))?;
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let kind = match extension.as_str() {
            "pdf" => "pdf",
            "txt" => "text",
            "ppt" | "pptx" | "pps" | "ppsx" => "presentation",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "tif" => "image",
            "mp4" | "mp3" | "wav" | "m4a" | "avi" | "mkv" | "mov" | "flac" | "webm" | "ogg" => {
                "media"
            }
            _ => anyhow::bail!(crate::i18n::text("暂不支持内置查看此格式")),
        };
        if kind == "presentation" {
            crate::presentation::validate(path)?;
        }
        let len = fs::metadata(path)?.len();
        ensure!(len > 0, crate::i18n::text("文件为空"));
        let pubkey = issuer.spki()?;
        let info = PackageInfo {
            custom_activation_window: true,
            version: 2,
            content_id: uuid::Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&pubkey),
            issuer_public_key: b64(&pubkey),
            display_name: path.file_name().unwrap().to_string_lossy().into(),
            kind: kind.into(),
            plain_length: len,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: crate::hardware::STABLE.into(),
            enrollment_public_key: issuer.enrollment_public()?,
        };
        let key = random::<32>();
        let temp = platform::root()
            .join("rust-admin-temp")
            .join(format!("{}.pkg", uuid::Uuid::new_v4()));
        fs::create_dir_all(temp.parent().unwrap())?;
        write_package(path, &temp, &info, &key)?;
        Ok((
            temp,
            Catalog {
                info,
                key_base64: b64(&key),
            },
        ))
    }
    fn command(&mut self, id: usize) -> Result<()> {
        match id {
            LANGUAGE_CHINESE => self.change_language(crate::i18n::Language::Chinese)?,
            LANGUAGE_ENGLISH => self.change_language(crate::i18n::Language::English)?,
            ACTIVATE => {
                if let Role::Viewer(info) = &self.role {
                    if info.kind == "presentation" && !crate::presentation::available() {
                        message(crate::i18n::text("请先安装并激活 Microsoft PowerPoint 桌面版，再导入 LIC。本次未使用或消耗授权文件。"));
                        return Ok(());
                    }
                }
                if let Some(path) = file_dialog(
                    self.hwnd,
                    false,
                    crate::i18n::text("请选择该设备的授权文件"),
                    crate::i18n::text("授权文件\0*.lic"),
                    "",
                ) {
                    let info = match &self.role {
                        Role::Viewer(info) => info.clone(),
                        _ => anyhow::bail!(crate::i18n::text("请在内容程序中导入密钥")),
                    };
                    self.device = Some(Device::for_info(&info)?);
                    let key = self.auth.as_ref().unwrap().lock().unwrap().import(
                        &path,
                        self.device.as_ref().unwrap(),
                        &self.exe,
                    )?;
                    self.open_viewer(key)?;
                }
            }
            CREATE => {
                if let Some(path) = file_dialog(
                    self.hwnd,
                    true,
                    crate::i18n::text("保存发行者加密密钥"),
                    crate::i18n::text("发行者密钥\0*.issuerkey"),
                    crate::i18n::text("发行者密钥.issuerkey"),
                ) {
                    if let Some(pass) = prompt(
                        self.hwnd,
                        crate::i18n::text("新密钥口令（至少 8 字符）"),
                        true,
                    ) {
                        ensure!(
                            prompt(self.hwnd, crate::i18n::text("再次输入口令"), true).as_deref()
                                == Some(&pass),
                            crate::i18n::text("两次口令不一致")
                        );
                        let issuer = Issuer::create()?;
                        issuer.save(&path, &pass)?;
                        self.issuer = Some(issuer);
                        self.catalog = None;
                        self.status(crate::i18n::text("加密密钥已创建，请立即备份。"));
                    }
                }
            }
            LOAD => {
                if let Some(path) = file_dialog(
                    self.hwnd,
                    false,
                    crate::i18n::text("恢复 / 解锁发行者密钥"),
                    crate::i18n::text("发行者密钥\0*.issuerkey"),
                    "",
                ) {
                    if let Some(pass) = prompt(self.hwnd, crate::i18n::text("密钥口令"), true) {
                        let issuer = Issuer::load(&path, &pass)?;
                        self.status(&crate::tr_format!(
                            "发行者已解锁：{}",
                            "Issuer unlocked: {}",
                            fingerprint(&issuer.spki()?)
                        ));
                        self.issuer = Some(issuer);
                        self.catalog = None;
                    }
                }
            }
            BACKUP => {
                let issuer = self
                    .issuer
                    .as_ref()
                    .context(crate::i18n::text("请先解锁密钥"))?;
                if let Some(path) = file_dialog(
                    self.hwnd,
                    true,
                    crate::i18n::text("保存加密备份"),
                    crate::i18n::text("发行者密钥\0*.issuerkey"),
                    crate::i18n::text("发行者备份.issuerkey"),
                ) {
                    if let Some(pass) = prompt(
                        self.hwnd,
                        crate::i18n::text("备份口令（至少 8 字符）"),
                        true,
                    ) {
                        issuer.save(&path, &pass)?;
                        ensure!(
                            Issuer::load(&path, &pass)?.spki()? == issuer.spki()?,
                            crate::i18n::text("备份指纹不一致")
                        );
                        self.status(crate::i18n::text("加密备份及公钥指纹校验成功。"));
                    }
                }
            }
            IMPORT => {
                ensure!(
                    self.issuer.is_some(),
                    crate::i18n::text("请先创建或解锁密钥")
                );
                if let Some(mut path)=file_dialog(self.hwnd,false,crate::i18n::text("导入并预览文件"),crate::i18n::text("支持的文件\0*.pdf;*.txt;*.png;*.jpg;*.mp4;*.mp3;*.wav;*.docx;*.pptx;*.doc;*.ppt;*.pps;*.ppsx\0所有文件\0*.*"),""){
                    let ext=path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
                    if ["docx","doc"].contains(&ext.as_str()){path=self.convert_office(&path)?;}
                    self.stop();let (temp,catalog)=self.prepare(&path)?;
                    let package=Package::header(fs::File::open(&temp)?,0,fs::metadata(&temp)?.len())?;let info=catalog.info.clone();
                    self.open_reader(Reader::new(package,unb64(&catalog.key_base64)?)?,None,&info)?;
                    self.temp.push(temp);self.input=Some(path);self.catalog=None;
                    self.status(if info.kind=="presentation" {if crate::presentation::available(){crate::i18n::text("PPT 原文件已载入，正在启动 PowerPoint 放映预览；生成 EXE 后需 LIC 授权。")}else{crate::i18n::text("PPT 原文件已载入，可直接加密生成 EXE；本机没有 PowerPoint，无法预览。接收端需安装 PowerPoint。")}}else{crate::i18n::text("预览已打开，请检查内容后生成 EXE。Word 仍转换为静态页面。")});
                }
            }
            BUILD => {
                let input = self
                    .input
                    .as_ref()
                    .context(crate::i18n::text("请先导入并预览文件"))?
                    .clone();
                if let Some(path) = file_dialog(
                    self.hwnd,
                    true,
                    crate::i18n::text("生成内容 EXE"),
                    crate::i18n::text("内容程序\0*.exe"),
                    &format!("{}.exe", input.file_stem().unwrap().to_string_lossy()),
                ) {
                    ensure!(
                        path != input && path != self.exe,
                        crate::i18n::text("不能覆盖原文件或管理员程序")
                    );
                    self.status(crate::i18n::text("正在加密并构建 EXE…"));
                    let (temp, catalog) = self.prepare(&input)?;
                    let result = build_exe(&self.exe, &temp, &path);
                    let _ = fs::remove_file(&temp);
                    result?;
                    self.issuer.as_ref().unwrap().catalog_save(
                        &PathBuf::from(format!("{}.olmeta", path.display())),
                        &catalog,
                    )?;
                    self.catalog = Some(catalog);
                    self.status(if crate::presentation::supported(&input.extension().unwrap_or_default().to_string_lossy().to_lowercase()) {crate::i18n::text("PPT 原文件已加密生成 EXE。只发送 EXE；.olmeta 留在管理员端。接收端需安装 PowerPoint，LIC 验证后仅放映。")} else {crate::i18n::text("EXE 已生成。只发送 EXE；.olmeta 请保留备份。视频授权后全屏单个循环。")});
                }
            }
            SELECT => {
                let issuer = self
                    .issuer
                    .as_ref()
                    .context(crate::i18n::text("请先解锁密钥"))?;
                if let Some(path) = file_dialog(
                    self.hwnd,
                    false,
                    crate::i18n::text("选择已有内容 EXE"),
                    crate::i18n::text("内容程序\0*.exe"),
                    "",
                ) {
                    let info = Package::open_exe(&path)?.info;
                    ensure!(
                        info.version == 2,
                        crate::i18n::text("请选择新需求版生成的内容包；旧内容请重新打包")
                    );
                    let mut meta = PathBuf::from(format!("{}.olmeta", path.display()));
                    if !meta.exists() {
                        meta = match file_dialog(
                            self.hwnd,
                            false,
                            crate::i18n::text("选择该包的管理员记录"),
                            crate::i18n::text("管理员记录\0*.olmeta"),
                            "",
                        ) {
                            Some(p) => p,
                            None => return Ok(()),
                        };
                    }
                    let c = issuer.catalog_load(&meta)?;
                    ensure!(c.info == info, crate::i18n::text("管理记录与内容 EXE 不符"));
                    self.stop();
                    self.input = None;
                    self.catalog = Some(c);
                    self.status(crate::i18n::text("已有内容包及管理员记录已载入。"));
                }
            }
            ISSUE => {
                ensure!(self.issuer.is_some(), crate::i18n::text("请先解锁密钥"));
                let c = self
                    .catalog
                    .as_ref()
                    .context(crate::i18n::text("请先生成或选择内容包"))?
                    .clone();
                let options = self.read_signing_form()?;
                let days = options.days;
                let window_seconds = options.seconds()?;
                if let Some(path) = file_dialog(
                    self.hwnd,
                    false,
                    crate::i18n::text("导入加密设备识别码"),
                    crate::i18n::text("加密设备信息\0*.json;*.josn\0旧版设备请求\0*.request"),
                    "",
                ) {
                    let (request, display) = if c.info.version == 2 {
                        let enrollment = crate::hardware::decrypt(
                            self.issuer.as_ref().unwrap(),
                            &c.info,
                            &crate::hardware::read_enrollment(&path)?,
                        )?;
                        (enrollment.device, enrollment.hardware.display()?)
                    } else {
                        (read_json::<DeviceRequest>(&path)?, String::new())
                    };
                    validate_request(&request)?;
                    if !display.is_empty() {
                        self.stop();
                        self.text = control(
                            self.hwnd,
                            "RICHEDIT50W",
                            &display,
                            0,
                            10,
                            110,
                            1000,
                            500,
                            ES_MULTILINE as u32 | ES_READONLY as u32 | WS_VSCROLL | WS_BORDER,
                            self.font,
                        );
                        self.layout();
                        self.status(crate::i18n::text("完整设备信息已解密。绑定排除 USB 移动盘和网卡连接状态，其他硬件变化需要重新授权。"));
                    }
                    if let Some(output) = file_dialog(
                        self.hwnd,
                        true,
                        crate::i18n::text("保存授权文件"),
                        crate::i18n::text("授权文件\0*.lic"),
                        crate::i18n::text("设备授权.lic"),
                    ) {
                        let accepted = unsafe {
                            MessageBoxW(self.hwnd,wide(&crate::tr_format!("设备：{}\n{}\n点击确认后开始首次导入倒计时。新授权不叠加旧期限。", "Device: {}\n{}\nConfirming starts the first-import window. A new license replaces the previous term.",request.device_id,options.summary())).as_ptr(),wide(crate::i18n::text("立即签发")).as_ptr(),MB_OKCANCEL)
                        };
                        if accepted == IDOK {
                            let license = issue_with_window(
                                self.issuer.as_ref().unwrap(),
                                &c,
                                &request,
                                days,
                                window_seconds,
                                now(),
                            )?;
                            atomic(&output, &encode(&license)?)?;
                            self.status(&crate::tr_format!("授权已签发：请在 {} {}内首次导入。使用期限 {} 天，从首次激活成功后计算。", "License issued: import within {} {}. Usage lasts {} days from first activation.",options.value,options.unit_caption(),days));
                        }
                    }
                }
            }
            DEVICE => {
                let target = self.exe.parent().unwrap().join("DeviceTool.exe");
                if target.exists() {
                    Command::new(target).spawn()?;
                } else {
                    Command::new(&self.exe).arg("--device").spawn()?;
                }
            }
            EXPORT => {
                let info = match &self.role {
                    Role::Viewer(info) => info.clone(),
                    Role::Device => {
                        let Some(content) = file_dialog(
                            self.hwnd,
                            false,
                            crate::i18n::text("选择收到的内容 EXE"),
                            crate::i18n::text("内容程序\0*.exe"),
                            "",
                        ) else {
                            return Ok(());
                        };
                        Package::open_exe(&content)?.info
                    }
                    _ => anyhow::bail!(crate::i18n::text("请在内容程序中获取设备识别码")),
                };
                self.status(crate::i18n::text(
                    "正在采集并加密完整硬件信息，不采集 IP 地址、网关或 DNS…",
                ));
                ensure!(
                    info.version == 2,
                    crate::i18n::text("旧内容需要在新需求版管理员中重新打包")
                );
                let device = Device::for_info(&info)?;
                let output = if info.version == 2 {
                    encode(&crate::hardware::export(&info, &device)?)?
                } else {
                    encode(&device.request()?)?
                };
                self.device = Some(device);
                if let Some(path) = file_dialog(
                    self.hwnd,
                    true,
                    crate::i18n::text("保存加密设备识别码"),
                    crate::i18n::text("加密设备信息\0*.json;*.josn"),
                    crate::i18n::text("设备识别码.json"),
                ) {
                    atomic(&path, &output)?;
                    self.status(crate::i18n::text("加密设备信息已导出，请发给管理员，随后通过“导入固态链密钥文件”导入收到的 .lic。"));
                }
            }
            PREV | NEXT => {
                if let Some(pdf) = &self.pdf {
                    let n = if id == PREV {
                        self.page - 1
                    } else {
                        self.page + 1
                    };
                    if n >= 0 && n < pdf.pages {
                        self.bitmap = Some(pdf.page(n)?);
                        self.page = n;
                        self.status(&crate::tr_format!(
                            "第 {} / {} 页",
                            "Page {} of {}",
                            n + 1,
                            pdf.pages
                        ));
                        unsafe {
                            InvalidateRect(self.hwnd, ptr::null(), 1);
                        }
                    }
                }
            }
            PLAY => {
                if let Some(v) = &self.video {
                    v.play()?;
                } else if let Role::Viewer(info) = &self.role {
                    if info.kind == "presentation" {
                        let device = self
                            .device
                            .as_ref()
                            .context(crate::i18n::text("请先授权"))?;
                        let key = self
                            .auth
                            .as_ref()
                            .context(crate::i18n::text("请先授权"))?
                            .lock()
                            .unwrap()
                            .resume(device)?
                            .context(crate::i18n::text("授权已失效"))?;
                        self.open_viewer(key)?;
                    }
                } else if matches!(self.role, Role::Administrator) {
                    if !crate::presentation::available() {
                        message(crate::i18n::text(
                            "本机未安装 PowerPoint，不能预览 PPT，但可以直接生成加密 EXE。",
                        ));
                        return Ok(());
                    }
                    if let Some(input) = self.input.clone() {
                        if crate::presentation::supported(
                            &input
                                .extension()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase(),
                        ) {
                            let (temp, catalog) = self.prepare(&input)?;
                            let package = Package::header(
                                fs::File::open(&temp)?,
                                0,
                                fs::metadata(&temp)?.len(),
                            )?;
                            self.open_reader(
                                Reader::new(package, unb64(&catalog.key_base64)?)?,
                                None,
                                &catalog.info,
                            )?;
                            self.temp.push(temp);
                            self.status(crate::i18n::text("正在启动 PowerPoint 放映预览。"));
                        }
                    }
                }
            }
            PAUSE => {
                if let Some(v) = &self.video {
                    v.pause();
                }
            }
            RENEW => {
                if let Some(a) = &self.auth {
                    a.lock().unwrap().active = false;
                }
                self.activation(crate::i18n::text("请选择新的授权文件"));
                self.command(ACTIVATE)?;
            }
            FULLSCREEN => {
                if self.fullscreen {
                    self.leave_fullscreen();
                } else {
                    self.fullscreen_requested = true;
                    self.fullscreen();
                }
            }
            CLOSE_PLAYER => {
                self.stop();
                unsafe {
                    DestroyWindow(self.hwnd);
                }
            }
            STARTUP_REPAIR => {
                if let Role::Viewer(info) = &self.role {
                    let target = crate::watchdog::enable_startup(&self.exe, info)?;
                    self.startup_notice = None;
                    self.status(&format!("登录自动播放已开启：{}", target.display()));
                }
            }
            INSTALL => {
                let info = match &self.role {
                    Role::Viewer(i) => Some(i),
                    _ => None,
                };
                let target = crate::install::install(&self.exe, info)?;
                self.status(&crate::tr_format!(
                    "已安装到 {}",
                    "Installed to {}",
                    target.display()
                ));
            }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<()> {
        self.refresh_signing_form();
        self.update_dock();
        if let Some(video) = &self.video {
            unsafe {
                if GetCapture() != self.seekbar {
                    SendMessageW(
                        self.seekbar,
                        TBM_SETPOS,
                        1,
                        (video.position() * 1000.0) as isize,
                    );
                }
            }
            set_text(
                self.media_clock,
                &format!("{} / {}", clock(video.time()), clock(video.length())),
            );
        }
        if let Some(a) = &self.auth {
            let mut s = a.lock().unwrap();
            if s.active {
                #[cfg(feature = "watchdog-test-mode")]
                let validation = if unsafe {
                    RemovePropW(self.hwnd, wide("RustTestExpireAuthorization").as_ptr())
                } != 0
                {
                    s.expire_for_test()
                } else {
                    s.validate()
                };
                #[cfg(not(feature = "watchdog-test-mode"))]
                let validation = s.validate();
                if let Err(e) = validation {
                    s.lock();
                    let msg = e.to_string();
                    drop(s);
                    if let Role::Viewer(info) = &self.role {
                        crate::watchdog::disable_startup(info)?;
                    }
                    self.activation(&msg);
                    return Ok(());
                }
                if now() - s.last_saved >= 5 {
                    s.save()?;
                }
            }
        }
        let mut enter = false;
        if let Some(presentation) = self.presentation.as_mut() {
            match presentation.poll() {
                Ok(crate::presentation::State::Ended) => {
                    self.presentation.take();
                    if let Some(mut watch) = self.watch.take() {
                        watch.stop();
                    }
                    self.status(crate::i18n::text("本次 PPT 放映已结束，不计故障。点击“开始放映”可再次放映；授权到期后不能重新打开。"));
                }
                Ok(_) => {}
                Err(e) => {
                    if self.watch.is_some() {
                        self.fail_playback();
                        return Ok(());
                    }
                    self.presentation.take();
                    self.status(&e.to_string());
                    message(&e.to_string());
                }
            }
        }
        if let Some(video) = &self.video {
            let media_time = video.time().max(0);
            if matches!(self.role, Role::Viewer(_)) && media_time + 500 < self.last_media_time {
                self.loop_count = self.loop_count.saturating_add(1);
            }
            self.last_media_time = media_time;
            unsafe {
                SetPropW(
                    self.hwnd,
                    wide("RustLoopCount").as_ptr(),
                    self.loop_count as isize,
                );
                SetPropW(
                    self.hwnd,
                    wide("RustLoopFallbacks").as_ptr(),
                    self.loop_fallbacks as isize,
                );
                SetPropW(
                    self.hwnd,
                    wide("RustMediaTime").as_ptr(),
                    video.time().max(0) as isize,
                );
                SetPropW(
                    self.hwnd,
                    wide("RustMediaState").as_ptr(),
                    video.state() as isize,
                );
            }
            if matches!(self.role, Role::Viewer(_))
                && self.fullscreen_requested
                && !self.fullscreen
                && video.has_video()
            {
                enter = true;
            }
            if video.state() == 6
                && matches!(self.role, Role::Viewer(_))
                && (self.fullscreen || video.has_video())
            {
                video.repeat()?;
                self.loop_fallbacks = self.loop_fallbacks.saturating_add(1);
                unsafe {
                    SetPropW(
                        self.hwnd,
                        wide("RustLoopCount").as_ptr(),
                        self.loop_count as isize,
                    );
                }
            }
            if video.state() == 7 {
                if self.watch.is_some() {
                    self.fail_playback();
                    return Ok(());
                }
                anyhow::bail!(crate::i18n::text("媒体无法解码，请检查格式或内容是否损坏"));
            }
        }
        if let Some(watch) = self.watch.as_mut() {
            if let Some(video) = &self.video {
                watch.pulse(
                    if video.state() == 4 {
                        crate::watchdog::Health::Paused
                    } else {
                        crate::watchdog::Health::Running
                    },
                    true,
                    video.state(),
                    video.time(),
                )?;
            } else {
                watch.pulse(crate::watchdog::Health::Running, false, 0, 0)?;
            }
        }
        if enter {
            self.fullscreen();
        }
        Ok(())
    }
    fn paint(&self) {
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let window_dc = BeginPaint(self.hwnd, &mut ps);
            let saved_dc = SaveDC(window_dc);
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut client);
            let dc = CreateCompatibleDC(window_dc);
            let frame =
                CreateCompatibleBitmap(window_dc, client.right.max(1), client.bottom.max(1));
            if dc == 0 || frame == 0 {
                if dc != 0 {
                    DeleteDC(dc);
                }
                if frame != 0 {
                    DeleteObject(frame);
                }
                FillRect(window_dc, &client, theme_brush());
                RestoreDC(window_dc, saved_dc);
                EndPaint(self.hwnd, &ps);
                return;
            }
            let old_frame = SelectObject(dc, frame);
            FillRect(
                dc,
                &client,
                if self.fullscreen {
                    GetStockObject(BLACK_BRUSH)
                } else {
                    theme_brush()
                },
            );
            if !self.fullscreen {
                draw_artwork(dc, &self.artwork, 0, 0, client.right, client.bottom);
            }
            if matches!(self.role, Role::Administrator) {
                let half = (client.right - 72) / 2;
                for x in [24, 48 + half] {
                    draw_panel(dc, x, 100, half, 260);
                }
                draw_label(
                    dc,
                    workspace_title_font(),
                    24,
                    22,
                    client.right - 48,
                    36,
                    crate::i18n::text("离线授权工作台"),
                    0x006B4155,
                );
                draw_label(
                    dc,
                    self.font,
                    24,
                    62,
                    client.right - 48,
                    26,
                    crate::i18n::text(
                        "先打包内容，再为设备签发授权。首次导入窗口与使用期限独立设置。",
                    ),
                    0x008F7E89,
                );
                draw_label(
                    dc,
                    self.font,
                    44,
                    114,
                    half - 40,
                    28,
                    crate::i18n::text("01  内容与发行密钥"),
                    0x007B536B,
                );
                draw_label(
                    dc,
                    self.font,
                    68 + half,
                    114,
                    half - 40,
                    28,
                    crate::i18n::text("02  授权设置与签发"),
                    0x007B536B,
                );
            }
            if !self.fullscreen
                && self.video.is_none()
                && self.pdf.is_none()
                && self.bitmap.is_none()
                && self.text == 0
            {
                if matches!(self.role, Role::Administrator) {
                    let card_x = client.right - 384;
                    let card_y = 390;
                    let card_h = (client.bottom - card_y - 90).clamp(150, 300);
                    draw_side_artwork(dc, &self.side_artwork, card_x, card_y, 360, card_h);
                }
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, 0x007B5B68);
                SelectObject(dc, self.font);
                let mut title = RECT {
                    left: 40,
                    top: if matches!(self.role, Role::Administrator) {
                        410
                    } else {
                        170
                    },
                    right: (client.right - 400).max(400),
                    bottom: if matches!(self.role, Role::Administrator) {
                        455
                    } else {
                        245
                    },
                };
                let label = wide(crate::i18n::text("离线授权 · 你的专属播放空间"));
                DrawTextW(dc, label.as_ptr(), -1, &mut title, DT_LEFT | DT_WORDBREAK);
                let mut subtitle = RECT {
                    left: 40,
                    top: if matches!(self.role, Role::Administrator) {
                        470
                    } else {
                        245
                    },
                    right: (client.right - 400).max(400),
                    bottom: if matches!(self.role, Role::Administrator) {
                        580
                    } else {
                        400
                    },
                };
                let label = wide(crate::i18n::text("VLC 播放内核  /  原文件加密\n视频默认全屏循环，移动鼠标到屏幕底部显示控制栏。\n空格 暂停/播放 · ←/→ 跳转10秒 · F11 全屏 · Esc 返回窗口"));
                DrawTextW(
                    dc,
                    label.as_ptr(),
                    -1,
                    &mut subtitle,
                    DT_LEFT | DT_WORDBREAK,
                );
            }
            if let Some(b) = &self.bitmap {
                let mut r: RECT = std::mem::zeroed();
                GetClientRect(self.hwnd, &mut r);
                let top = if matches!(self.role, Role::Administrator) {
                    if self.pdf.is_some() {
                        436
                    } else {
                        386
                    }
                } else {
                    110
                };
                let w = r.right;
                let h = (r.bottom - top - 65).max(1);
                let ratio = (w as f64 / b.width as f64).min(h as f64 / b.height as f64);
                let dw = (b.width as f64 * ratio) as i32;
                let dh = (b.height as f64 * ratio) as i32;
                let mut info: BITMAPINFO = std::mem::zeroed();
                info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = b.width;
                info.bmiHeader.biHeight = -b.height;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB;
                StretchDIBits(
                    dc,
                    (w - dw) / 2,
                    top + (h - dh) / 2,
                    dw,
                    dh,
                    0,
                    0,
                    b.width,
                    b.height,
                    b.bgra.as_ptr().cast(),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
            }
            BitBlt(
                window_dc,
                0,
                0,
                client.right,
                client.bottom,
                dc,
                0,
                0,
                SRCCOPY,
            );
            SelectObject(dc, old_frame);
            DeleteDC(dc);
            let previous = RemovePropW(self.hwnd, wide("OfflinePaintFrame").as_ptr());
            if SetPropW(self.hwnd, wide("OfflinePaintFrame").as_ptr(), frame) == 0 {
                DeleteObject(frame);
            }
            if previous != 0 {
                DeleteObject(previous);
            }
            RestoreDC(window_dc, saved_dc);
            EndPaint(self.hwnd, &ps);
        }
    }
    fn convert_office(&mut self, input: &Path) -> Result<PathBuf> {
        let ext = input.extension().unwrap().to_string_lossy().to_lowercase();
        ensure!(
            fs::metadata(input)?.len() > 0,
            crate::i18n::text("Office 文件为空")
        );
        if ext == "docx" || ext == "pptx" {
            let mut zip = zip::ZipArchive::new(fs::File::open(input)?)
                .context(crate::i18n::text("Office 文档不是有效 ZIP"))?;
            let expected = if ext == "docx" {
                "word/document.xml"
            } else {
                "ppt/presentation.xml"
            };
            ensure!(
                zip.by_name("[Content_Types].xml").is_ok() && zip.by_name(expected).is_ok(),
                crate::i18n::text("Office 文档结构无效")
            );
        } else {
            let mut magic = [0; 8];
            fs::File::open(input)?.read_exact(&mut magic)?;
            ensure!(
                magic == [208, 207, 17, 224, 161, 177, 26, 225],
                crate::i18n::text("DOC/PPT 文件头无效")
            );
        }
        let mut soffice = PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_default())
            .join("LibreOffice/program/soffice.exe");
        if !soffice.exists() {
            let installed =
                PathBuf::from(std::env::var_os("ProgramFiles(x86)").unwrap_or_default())
                    .join("LibreOffice/program/soffice.exe");
            if installed.exists() {
                soffice = installed;
            }
        }
        if !soffice.exists() {
            soffice = match file_dialog(
                self.hwnd,
                false,
                crate::i18n::text("选择已安装 LibreOffice 的 soffice.exe"),
                "LibreOffice\0soffice.exe",
                "",
            ) {
                Some(p) => p,
                None => anyhow::bail!(crate::i18n::text("转换需要管理员电脑安装 LibreOffice")),
            };
        }
        let com = soffice.with_extension("com");
        if com.exists() {
            soffice = com;
        }
        let dir = platform::root()
            .join("rust-office")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir)?;
        self.temp.push(dir.clone());
        let profile = dir.join("profile").to_string_lossy().replace('\\', "/");
        let uri = format!(
            "file:///{}",
            profile
                .bytes()
                .map(|c| if c.is_ascii_alphanumeric() || b"/:_-".contains(&c) {
                    (c as char).to_string()
                } else {
                    format!("%{c:02X}")
                })
                .collect::<String>()
        );
        self.status(crate::i18n::text("正在通过 LibreOffice 转换为静态 PDF…"));
        let mut child = Command::new(soffice)
            .arg(format!("-env:UserInstallation={uri}"))
            .args(["--headless", "--convert-to", "pdf", "--outdir"])
            .arg(&dir)
            .arg(input)
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(
                    status.success(),
                    crate::i18n::text("Office 转换程序退出失败")
                );
                break;
            }
            if start.elapsed() > Duration::from_secs(180) {
                let _ = child.kill();
                anyhow::bail!(crate::i18n::text("Office 转换超时"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let pdf = dir.join(input.file_stem().unwrap()).with_extension("pdf");
        ensure!(pdf.exists(), crate::i18n::text("Office 转换没有生成 PDF"));
        Ok(pdf)
    }
}
unsafe fn paint_button(l: isize) -> isize {
    let item = &*(l as *const DRAWITEMSTRUCT);
    let accent = matches!(item.CtlID as usize, BUILD | ISSUE | ACTIVATE | PLAY);
    let selected = item.itemState & ODS_SELECTED != 0;
    let disabled = item.itemState & ODS_DISABLED != 0;
    let color = if disabled {
        0x00EEE9EE
    } else if accent {
        if selected {
            0x00834E71
        } else {
            0x00A76E8D
        }
    } else if selected {
        0x00E9DAE3
    } else {
        0x00FFFFFF
    };
    FillRect(item.hDC, &item.rcItem, theme_brush());
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(PS_SOLID, 1, 0x00DBC4D2);
    let old_brush = SelectObject(item.hDC, brush);
    let old_pen = SelectObject(item.hDC, pen);
    RoundRect(
        item.hDC,
        item.rcItem.left,
        item.rcItem.top,
        item.rcItem.right,
        item.rcItem.bottom,
        12,
        12,
    );
    SelectObject(item.hDC, old_brush);
    SelectObject(item.hDC, old_pen);
    DeleteObject(brush);
    DeleteObject(pen);
    let old_font = SelectObject(item.hDC, SendMessageW(item.hwndItem, WM_GETFONT, 0, 0));
    SetBkMode(item.hDC, TRANSPARENT as i32);
    SetTextColor(
        item.hDC,
        if disabled {
            0x00AA99A5
        } else if accent {
            0x00FFFFFF
        } else {
            0x006B4155
        },
    );
    let mut rect = item.rcItem;
    let caption = wide(&text(item.hwndItem));
    DrawTextW(
        item.hDC,
        caption.as_ptr(),
        -1,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    if item.itemState & ODS_FOCUS != 0 {
        rect.left += 3;
        rect.top += 3;
        rect.right -= 3;
        rect.bottom -= 3;
        DrawFocusRect(item.hDC, &rect);
    }
    SelectObject(item.hDC, old_font);
    1
}

unsafe fn dispatch(h: isize, m: u32, w: usize, l: isize) -> isize {
    if m == WM_NCCREATE {
        let c = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(h, GWLP_USERDATA, c.lpCreateParams as isize);
    }
    let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut App;
    if !p.is_null() {
        let app = &mut *p;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Option<isize>> {
                match m {
                    WM_GETMINMAXINFO => {
                        let limits = &mut *(l as *mut MINMAXINFO);
                        limits.ptMinTrackSize.x = if matches!(app.role, Role::Administrator) {
                            1120
                        } else {
                            800
                        };
                        limits.ptMinTrackSize.y = if matches!(app.role, Role::Administrator) {
                            760
                        } else {
                            480
                        };
                        Ok(Some(0))
                    }
                    WM_CTLCOLORSTATIC => {
                        SetBkMode(w as isize, TRANSPARENT as i32);
                        let invalid = l == app.signing_summary && app.read_signing_form().is_err();
                        SetTextColor(w as isize, if invalid { 0x004343B2 } else { 0x00453735 });
                        let form = matches!(GetDlgCtrlID(l), 21 | 24 | 25)
                            && matches!(app.role, Role::Administrator);
                        return Ok(Some(if form {
                            GetStockObject(WHITE_BRUSH)
                        } else {
                            theme_brush()
                        }));
                    }
                    WM_CTLCOLORBTN => {
                        SetBkMode(w as isize, TRANSPARENT as i32);
                        return Ok(Some(theme_brush()));
                    }
                    WM_COMMAND => {
                        let id = w & 0xffff;
                        let notification = (w >> 16) & 0xffff;
                        if matches!(id, 20 | 22 | 23) && matches!(notification, 1 | 768) {
                            app.refresh_signing_form();
                            return Ok(Some(0));
                        }
                        app.command(id)?;
                        app.refresh_signing_form();
                        Ok(Some(0))
                    }
                    WM_HSCROLL => {
                        if let Some(v) = &app.video {
                            if l == app.seekbar {
                                v.seek(SendMessageW(l, TBM_GETPOS, 0, 0) as f32 / 1000.0)?;
                            } else if l == app.volbar {
                                v.volume(SendMessageW(l, TBM_GETPOS, 0, 0) as i32);
                            }
                        }
                        Ok(Some(0))
                    }
                    WM_TIMER => {
                        app.tick()?;
                        Ok(Some(0))
                    }
                    WM_SIZE => {
                        app.layout();
                        Ok(Some(0))
                    }
                    WM_PAINT => {
                        app.paint();
                        Ok(Some(0))
                    }
                    WM_ERASEBKGND if app.fullscreen && app.video.is_some() => {
                        let mut rect: RECT = std::mem::zeroed();
                        GetClientRect(h, &mut rect);
                        FillRect(w as isize, &rect, GetStockObject(BLACK_BRUSH));
                        Ok(Some(1))
                    }
                    WM_KEYDOWN if app.media_key(w)? => Ok(Some(0)),
                    WM_KEYDOWN if w == 27 => {
                        app.stop();
                        DestroyWindow(h);
                        Ok(Some(0))
                    }
                    WM_CLOSE => {
                        app.stop();
                        DestroyWindow(h);
                        Ok(Some(0))
                    }
                    WM_DESTROY => {
                        PostQuitMessage(0);
                        Ok(Some(0))
                    }
                    _ => Ok(None),
                }
            }));
        match result {
            Ok(Ok(Some(n))) => return n,
            Ok(Ok(None)) => {}
            Ok(Err(e)) => {
                if matches!(app.role, Role::Viewer(_)) {
                    if let Some(a) = &app.auth {
                        a.lock().unwrap().lock();
                    }
                    app.activation(&e.to_string());
                } else {
                    app.status(&e.to_string());
                    message(&e.to_string());
                }
                return 0;
            }
            Err(_) => {
                message(crate::i18n::text("程序遇到内部异常，操作已停止。"));
                return 0;
            }
        }
    }
    DefWindowProcW(h, m, w, l)
}

unsafe extern "system" fn wndproc(h: isize, m: u32, w: usize, l: isize) -> isize {
    if m == WM_NCCREATE {
        let c = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(h, GWLP_USERDATA, c.lpCreateParams as isize);
    }
    if m == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    // Rendering notifications may arrive inside modal dialogs or MoveWindow while
    // dispatch owns &mut App. Render independently; never alias that reference.
    if m == WM_DRAWITEM {
        return paint_button(l);
    }
    if m == WM_ERASEBKGND {
        return 1;
    }
    if m == WM_NCDESTROY {
        let frame = RemovePropW(h, wide("OfflinePaintFrame").as_ptr());
        if frame != 0 {
            DeleteObject(frame);
        }
    }
    let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut App;
    if !p.is_null() && (*p).busy {
        if m == WM_PAINT {
            return paint_cached_frame(h);
        }
        if m == WM_CTLCOLORSTATIC || m == WM_CTLCOLORBTN {
            SetBkMode(w as isize, TRANSPARENT as i32);
            SetTextColor(w as isize, 0x00453735);
            return if matches!(GetDlgCtrlID(l), 21 | 24 | 25) {
                GetStockObject(WHITE_BRUSH)
            } else {
                theme_brush()
            };
        }
    }
    if p.is_null()
        || (*p).busy
        || !matches!(
            m,
            WM_COMMAND
                | WM_HSCROLL
                | WM_TIMER
                | WM_SIZE
                | WM_PAINT
                | WM_ERASEBKGND
                | WM_KEYDOWN
                | WM_CLOSE
                | WM_CTLCOLORSTATIC
                | WM_CTLCOLORBTN
                | WM_DRAWITEM
                | WM_GETMINMAXINFO
        )
    {
        return DefWindowProcW(h, m, w, l);
    }
    // Win32 can synchronously call this procedure during window operations.
    // Never create a second mutable App reference while dispatch is active.
    (*p).busy = true;
    let result = dispatch(h, m, w, l);
    (*p).busy = false;
    if matches!(m, WM_COMMAND | WM_SIZE) {
        RedrawWindow(h, ptr::null(), 0, RDW_INVALIDATE | RDW_ALLCHILDREN);
    }
    result
}

unsafe fn paint_cached_frame(h: isize) -> isize {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(h, &mut ps);
    FillRect(dc, &ps.rcPaint, theme_brush());
    let frame = GetPropW(h, wide("OfflinePaintFrame").as_ptr());
    if frame != 0 {
        let memory = CreateCompatibleDC(dc);
        if memory != 0 {
            let old = SelectObject(memory, frame);
            let mut bitmap: BITMAP = std::mem::zeroed();
            if GetObjectW(
                frame,
                std::mem::size_of::<BITMAP>() as i32,
                (&mut bitmap as *mut BITMAP).cast(),
            ) != 0
            {
                BitBlt(
                    dc,
                    0,
                    0,
                    bitmap.bmWidth,
                    bitmap.bmHeight,
                    memory,
                    0,
                    0,
                    SRCCOPY,
                );
            }
            SelectObject(memory, old);
            DeleteDC(memory);
        }
    }
    EndPaint(h, &ps);
    0
}

fn clock(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
fn theme_brush() -> isize {
    static BRUSH: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    *BRUSH.get_or_init(|| unsafe { CreateSolidBrush(0x00FCF6F8) })
}
fn load_artwork() -> Result<Bitmap> {
    let img = image::load_from_memory(include_bytes!("../assets/player-art.png"))?.to_rgba8();
    let (width, height) = img.dimensions();
    let mut bgra = img.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        // Composite the artwork once at 14% opacity over the workspace color.
        for (channel, base) in pixel[..3].iter_mut().zip([252u16, 246, 248]) {
            *channel = ((*channel as u16 * 14 + base * 86) / 100) as u8;
        }
        pixel[3] = 255;
    }
    Ok(Bitmap {
        width: width as i32,
        height: height as i32,
        bgra,
    })
}
fn load_side_artwork() -> Result<Bitmap> {
    let image = image::load_from_memory(include_bytes!("../assets/dragon-side.png"))?
        .thumbnail(768, 768)
        .to_rgba8();
    let (width, height) = image.dimensions();
    let mut bgra = image.into_raw();
    // Premultiplied alpha for AC_SRC_ALPHA: feather into the actual canvas,
    // rather than fading to a solid rectangle or changing window opacity.
    let feather = width.min(height) as f32 * 0.16;
    for (index, pixel) in bgra.chunks_exact_mut(4).enumerate() {
        let x = index as u32 % width;
        let y = index as u32 / width;
        let distance = x.min(width - 1 - x).min(y.min(height - 1 - y)) as f32;
        let t = (distance / feather).clamp(0.0, 1.0);
        let alpha = (pixel[3] as f32 * t * t * (3.0 - 2.0 * t)).round() as u8;
        pixel.swap(0, 2);
        for channel in &mut pixel[..3] {
            *channel = ((*channel as u16 * alpha as u16 + 127) / 255) as u8;
        }
        pixel[3] = alpha;
    }
    Ok(Bitmap {
        width: width as i32,
        height: height as i32,
        bgra,
    })
}

unsafe fn draw_artwork(dc: isize, b: &Bitmap, x: i32, y: i32, w: i32, h: i32) {
    let scale = (w as f64 / b.width as f64).max(h as f64 / b.height as f64);
    let dw = (b.width as f64 * scale) as i32;
    let dh = (b.height as f64 * scale) as i32;
    let x = x + (w - dw) / 2;
    let y = y + (h - dh) / 2;
    let w = dw;
    let h = dh;
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = b.width;
    info.bmiHeader.biHeight = -b.height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    SetStretchBltMode(dc, HALFTONE);
    StretchDIBits(
        dc,
        x,
        y,
        w,
        h,
        0,
        0,
        b.width,
        b.height,
        b.bgra.as_ptr().cast(),
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
unsafe fn draw_side_artwork(dc: isize, b: &Bitmap, x: i32, y: i32, w: i32, h: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let scale = (w as f64 / b.width as f64).min(h as f64 / b.height as f64);
    let dw = (b.width as f64 * scale).max(1.0) as i32;
    let dh = (b.height as f64 * scale).max(1.0) as i32;
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = b.width;
    info.bmiHeader.biHeight = -b.height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut bits = ptr::null_mut();
    let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, 0, 0);
    if bitmap == 0 {
        return;
    }
    let memory = CreateCompatibleDC(dc);
    if memory != 0 && !bits.is_null() {
        ptr::copy_nonoverlapping(b.bgra.as_ptr(), bits.cast(), b.bgra.len());
        let previous = SelectObject(memory, bitmap);
        AlphaBlend(
            dc,
            x + (w - dw) / 2,
            y + (h - dh) / 2,
            dw,
            dh,
            memory,
            0,
            0,
            b.width,
            b.height,
            BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            },
        );
        SelectObject(memory, previous);
    }
    if memory != 0 {
        DeleteDC(memory);
    }
    DeleteObject(bitmap);
}

unsafe fn draw_label(
    dc: isize,
    font: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &str,
    color: u32,
) {
    SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let caption = wide(label);
    DrawTextW(dc, caption.as_ptr(), -1, &mut rect, DT_LEFT | DT_WORDBREAK);
}
unsafe fn draw_panel(dc: isize, x: i32, y: i32, w: i32, h: i32) {
    let brush = CreateSolidBrush(0x00FFFFFF);
    let pen = CreatePen(PS_SOLID, 1, 0x00EADDE5);
    let old_b = SelectObject(dc, brush);
    let old_p = SelectObject(dc, pen);
    RoundRect(dc, x, y, x + w, y + h, 20, 20);
    SelectObject(dc, old_b);
    SelectObject(dc, old_p);
    DeleteObject(brush);
    DeleteObject(pen);
}

fn workspace_title_font() -> isize {
    static FONT: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    *FONT.get_or_init(|| unsafe {
        CreateFontW(
            -26,
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            0,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        )
    })
}
