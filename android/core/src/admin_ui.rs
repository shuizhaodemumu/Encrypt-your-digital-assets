use crate::admin_theme::{rounded, Theme};
use anyhow::{bail, ensure, Context, Result};
use encrypt_video::{issue, pack, Issuer};
use std::{
    path::PathBuf,
    ptr::null,
    sync::mpsc::{self, Receiver},
    thread,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{
            Dialogs::*, DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED,
            ODT_BUTTON,
        },
        Input::KeyboardAndMouse::EnableWindow,
        WindowsAndMessaging::*,
    },
};
use zeroize::Zeroizing;

const PICK_KEY: usize = 101;
const NEW_KEY: usize = 102;
const PICK_VIDEO: usize = 103;
const PACK: usize = 104;
const PICK_PACKAGE: usize = 105;
const PICK_REQUEST: usize = 106;
const ISSUE: usize = 107;
const HELP: usize = 108;
const LANGUAGE: usize = 109;
const TITLE: &str = "安卓视频授权管理员 · 0.1.0 预览版";
const SS_OWNERDRAW: u32 = 13;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
enum Done {
    Key(PathBuf),
    Package(PathBuf),
    License(PathBuf),
}
struct Ui {
    window: HWND,
    key: HWND,
    password: HWND,
    video: HWND,
    package: HWND,
    request: HWND,
    days: HWND,
    seconds: HWND,
    status: HWND,
    controls: Vec<HWND>,
    font: HFONT,
    title_font: HFONT,
    background: HBRUSH,
    theme: Theme,
    unit: HWND,
    summary: HWND,
    english: bool,
    translations: Vec<(HWND, String)>,
    pending: Option<Receiver<Result<Done>>>,
}
impl Ui {
    unsafe fn control(
        &mut self,
        class: &str,
        text: &str,
        style: u32,
        id: usize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> HWND {
        let hwnd = CreateWindowExW(
            if class == "EDIT" { WS_EX_CLIENTEDGE } else { 0 },
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            self.window,
            id as _,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(hwnd, WM_SETFONT, self.font as usize, 1);
        if class == "STATIC" || class == "BUTTON" {
            self.translations.push((hwnd, text.to_string()));
        }
        if class == "BUTTON" || class == "EDIT" || class == "COMBOBOX" {
            self.controls.push(hwnd);
        }
        hwnd
    }
    unsafe fn label(&mut self, s: &str, x: i32, y: i32, w: i32, h: i32) -> HWND {
        self.control("STATIC", s, SS_OWNERDRAW as u32, 0, x, y, w, h)
    }
    unsafe fn button(&mut self, s: &str, id: usize, x: i32, y: i32, w: i32) -> HWND {
        self.control(
            "BUTTON",
            s,
            WS_TABSTOP | BS_OWNERDRAW as u32,
            id,
            x,
            y,
            w,
            40,
        )
    }
    unsafe fn path(&mut self, x: i32, y: i32, width: i32) -> HWND {
        self.control(
            "EDIT",
            "尚未选择",
            ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
            0,
            x,
            y,
            width,
            30,
        )
    }
    unsafe fn build(&mut self) {
        let t = self.label("Encrypt your digital assets", 28, 24, 850, 38);
        SendMessageW(t, WM_SETFONT, self.title_font as usize, 1);
        self.button("中文 / English", LANGUAGE, 960, 24, 164);
        self.label("安卓 · 离线视频授权工作台", 28, 84, 1000, 28);
        self.label(
            "先加密视频，再为安卓设备签发授权。首次导入窗口与使用期限独立设置。",
            28,
            126,
            1096,
            28,
        );
        self.label("01  内容与发行密钥", 50, 184, 493, 28);
        self.key = self.path(50, 230, 493);
        self.button("新建发行密钥", NEW_KEY, 50, 270, 240);
        self.button("选择已有密钥", PICK_KEY, 302, 270, 241);
        self.label("密钥口令", 50, 334, 105, 26);
        self.password = self.control(
            "EDIT",
            "",
            WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
            0,
            166,
            328,
            377,
            32,
        );
        SendMessageW(self.password, EM_SETLIMITTEXT, 1024, 0);
        self.video = self.path(50, 378, 493);
        self.button("选择原视频", PICK_VIDEO, 50, 420, 493);
        self.button("生成安卓加密视频 .eav", PACK, 50, 476, 493);
        self.label(
            "视频全屏循环 · 离线 LIC 验证\n密钥口令用于创建或解锁；管理员元数据请自行备份。",
            50,
            550,
            493,
            52,
        );
        self.label("02  授权设置与签发", 613, 184, 493, 28);
        self.package = self.path(613, 230, 493);
        self.button("选择安卓加密视频 .eav", PICK_PACKAGE, 613, 270, 493);
        self.request = self.path(613, 324, 493);
        self.button("选择安卓设备识别文件 .json", PICK_REQUEST, 613, 364, 493);
        self.label("授权天数", 613, 428, 165, 26);
        self.days = self.control(
            "EDIT",
            "30",
            WS_TABSTOP | ES_NUMBER as u32,
            201,
            790,
            422,
            316,
            34,
        );
        self.label("首次导入窗口", 613, 477, 165, 26);
        self.seconds = self.control(
            "EDIT",
            "60",
            WS_TABSTOP | ES_NUMBER as u32,
            202,
            790,
            471,
            186,
            34,
        );
        self.unit = self.control(
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            203,
            990,
            471,
            116,
            200,
        );
        self.units(1);
        self.summary = self.label("", 613, 518, 493, 44);
        self.period_summary();
        self.button("核对并签发安卓 LIC", ISSUE, 613, 566, 493);
        self.label("离线授权 · 你的专属播放空间", 40, 646, 750, 28);
        self.label("本工具只处理安卓视频。Windows 授权请使用原管理员。\n发行密钥、内容包和 LIC 分开管理，不跨平台混用。",40,690,750,52);
        self.button("安卓使用说明", HELP, 40, 746, 240);
        self.status = self.label(
            "请选择或新建安卓发行密钥。口令仅在本次进程中使用。",
            20,
            798,
            1112,
            34,
        );
    }
    unsafe fn units(&self, selected: usize) {
        SendMessageW(self.unit, CB_RESETCONTENT, 0, 0);
        for text in if self.english {
            ["Seconds", "Minutes", "Hours", "Days"]
        } else {
            ["秒", "分钟", "小时", "天"]
        } {
            SendMessageW(self.unit, CB_ADDSTRING, 0, wide(text).as_ptr() as isize);
        }
        SendMessageW(self.unit, CB_SETCURSEL, selected, 0);
    }
    unsafe fn period(&self) -> Result<(u64, u64)> {
        let index = SendMessageW(self.unit, CB_GETCURSEL, 0, 0) as usize;
        let value: u64 = self
            .get(self.seconds)
            .trim()
            .parse()
            .context("首次导入窗口必须是正整数")?;
        let seconds = value
            .checked_mul(*[1, 60, 3600, 86400].get(index).context("请选择时间单位")?)
            .context("首次导入窗口过大")?;
        validate_period(&self.get(self.days), &seconds.to_string())
    }
    unsafe fn period_summary(&self) {
        if self.summary == 0 {
            return;
        }
        let text = match self.period() {
            Ok((days, seconds)) => {
                if self.english {
                    format!("First import: within {seconds} seconds of issuance.\nUsage: {days} days from first activation.")
                } else {
                    format!(
                        "首次导入：签发后 {seconds} 秒内。\n使用期限：首次激活成功后 {days} 天。"
                    )
                }
            }
            Err(_) => {
                if self.english {
                    "Enter a valid duration and import window.".into()
                } else {
                    "请输入有效的授权天数和首次导入窗口。".into()
                }
            }
        };
        SetWindowTextW(self.summary, wide(&text).as_ptr());
    }
    unsafe fn toggle_language(&mut self) {
        self.english = !self.english;
        for (h, text) in &self.translations {
            SetWindowTextW(
                *h,
                wide(if self.english { english(text) } else { text }).as_ptr(),
            );
        }
        let selected = SendMessageW(self.unit, CB_GETCURSEL, 0, 0).max(0) as usize;
        self.units(selected);
        self.period_summary();
        InvalidateRect(self.window, null(), 0);
    }
    unsafe fn draw_item(&self, d: &DRAWITEMSTRUCT) {
        let mut origin = POINT { x: 0, y: 0 };
        MapWindowPoints(d.hwndItem, self.window, &mut origin, 1);
        self.theme
            .copy(d.hDC, origin.x, origin.y, d.rcItem.right, d.rcItem.bottom);
        let mut rect = d.rcItem;
        let text = self.get(d.hwndItem);
        let is_button = d.CtlType == ODT_BUTTON;
        let disabled = is_button && d.itemState & ODS_DISABLED != 0;
        let primary = d.CtlID as usize == PACK || d.CtlID as usize == ISSUE;
        let fill = if disabled {
            0x00f0e9ee
        } else if primary {
            if d.itemState & ODS_SELECTED != 0 {
                0x008c5075
            } else {
                0x00a76487
            }
        } else {
            0x00ffffff
        };
        let color = if disabled {
            0x009e939e
        } else if primary && is_button {
            0xffffff
        } else {
            0x006f4059
        };
        if is_button {
            rounded(d.hDC, &rect, fill, 0x00e4ccd9, 14);
            rect.left += 8;
            rect.right -= 8;
        }
        let font = SendMessageW(d.hwndItem, WM_GETFONT, 0, 0);
        SelectObject(d.hDC, if font == 0 { self.font } else { font });
        SetBkMode(d.hDC, TRANSPARENT as i32);
        SetTextColor(d.hDC, color);
        DrawTextW(
            d.hDC,
            wide(&text).as_ptr(),
            -1,
            &mut rect,
            DT_NOPREFIX
                | if is_button {
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE
                } else {
                    DT_LEFT | DT_WORDBREAK
                },
        );
        if is_button && d.itemState & ODS_FOCUS != 0 {
            InflateRect(&mut rect, -3, -4);
            DrawFocusRect(d.hDC, &rect);
        }
    }
    unsafe fn say(&self, s: &str) {
        SetWindowTextW(self.status, wide(s).as_ptr());
    }
    unsafe fn get(&self, h: HWND) -> String {
        let n = GetWindowTextLengthW(h);
        let mut b = vec![0u16; n as usize + 1];
        let read = GetWindowTextW(h, b.as_mut_ptr(), b.len() as i32);
        String::from_utf16_lossy(&b[..read as usize])
    }
    unsafe fn selected(&self, h: HWND, label: &str) -> Result<PathBuf> {
        let s = self.get(h);
        ensure!(!s.is_empty() && s != "尚未选择", "请先选择{label}");
        let p = PathBuf::from(s);
        ensure!(p.is_file(), "{label}不存在或无法访问");
        Ok(p)
    }
    unsafe fn secret(&self) -> Result<Zeroizing<String>> {
        let s = Zeroizing::new(self.get(self.password));
        ensure!(s.chars().count() >= 8, "请输入至少 8 个字符的发行密钥口令");
        Ok(s)
    }
    unsafe fn start(&mut self, desc: &str, job: impl FnOnce() -> Result<Done> + Send + 'static) {
        self.say(desc);
        for &h in &self.controls {
            EnableWindow(h, 0);
        }
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        thread::spawn(move || {
            let result = job();
            let _ = tx.send(result);
        });
    }
    unsafe fn poll(&mut self) {
        let result = match self.pending.as_ref().map(|r| r.try_recv()) {
            Some(Ok(v)) => v,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Err(anyhow::anyhow!("任务意外退出，请重试"))
            }
            _ => return,
        };
        self.pending = None;
        for &h in &self.controls {
            EnableWindow(h, 1);
        }
        match result {
            Ok(Done::Key(p)) => {
                SetWindowTextW(self.key, wide(&p.to_string_lossy()).as_ptr());
                self.say("发行密钥已保存。请备份密钥和口令，再选择要加密的视频。");
            }
            Ok(Done::Package(p)) => {
                SetWindowTextW(self.package, wide(&p.to_string_lossy()).as_ptr());
                self.say(&format!(
                    "加密成功：{}\r\n将 .eav 发给手机，请保留同目录 .eav.adminmeta。",
                    p.display()
                ));
            }
            Ok(Done::License(p)) => {
                self.say(&format!(
                    "授权已签发：{}\r\n请在首次导入窗口内发送给对应手机并导入。",
                    p.display()
                ));
                MessageBoxW(
                    self.window,
                    wide("LIC 已保存。请发送给对应的安卓手机，导入成功后即可播放。").as_ptr(),
                    wide("安卓授权已签发").as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            Err(e) => {
                self.say(&format!("未完成：{e:#}"));
                MessageBoxW(
                    self.window,
                    wide(&format!("{e:#}")).as_ptr(),
                    wide("请检查后重试").as_ptr(),
                    MB_OK | MB_ICONWARNING,
                );
            }
        }
    }
    unsafe fn action(&mut self, id: usize) -> Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        match id {
            LANGUAGE => self.toggle_language(),
            PICK_KEY | PICK_VIDEO | PICK_PACKAGE | PICK_REQUEST => {
                let (filter, title, target) = match id {
                    PICK_KEY => (
                        "安卓发行密钥\0*.issuerkey\0所有文件\0*.*\0",
                        "选择安卓发行密钥",
                        self.key,
                    ),
                    PICK_VIDEO => (
                        "视频文件\0*.mp4;*.mkv;*.avi;*.mov;*.webm;*.m4v;*.ts\0所有文件\0*.*\0",
                        "选择原视频",
                        self.video,
                    ),
                    PICK_PACKAGE => (
                        "安卓加密视频\0*.eav\0",
                        "选择安卓 .eav 内容包",
                        self.package,
                    ),
                    _ => (
                        "设备请求 JSON\0*.json\0",
                        "选择手机导出的加密设备 JSON",
                        self.request,
                    ),
                };
                if let Some(p) = dialog(self.window, false, title, filter, "")? {
                    SetWindowTextW(target, wide(&p.to_string_lossy()).as_ptr());
                    self.say("已选择文件。安卓授权只使用本工具生成的密钥和内容包。");
                }
            }
            NEW_KEY => {
                let password = self.secret()?;
                if let Some(p) = dialog(
                    self.window,
                    true,
                    "保存新的安卓发行密钥",
                    "安卓发行密钥\0*.issuerkey\0",
                    "issuerkey",
                )? {
                    ensure!(!p.exists(), "请使用新的文件名，不覆盖已有密钥");
                    self.start("正在生成安卓发行密钥，请稍候…", move || {
                        Issuer::create()?.save(&p, &password)?;
                        Ok(Done::Key(p))
                    });
                }
            }
            PACK => {
                let k = self.selected(self.key, "安卓发行密钥")?;
                let password = self.secret()?;
                let video = self.selected(self.video, "原视频")?;
                if let Some(p) = dialog(
                    self.window,
                    true,
                    "保存安卓加密视频",
                    "安卓加密视频\0*.eav\0",
                    "eav",
                )? {
                    ensure!(
                        !p.exists() && !p.with_extension("eav.adminmeta").exists(),
                        "输出文件或管理员元数据已存在，请使用新文件名"
                    );
                    self.start(
                        "正在加密视频，请稍候。此时不要关闭窗口…",
                        move || {
                            pack(&Issuer::load(&k, &password)?, &video, &p)?;
                            Ok(Done::Package(p))
                        },
                    );
                }
            }
            ISSUE => {
                let k = self.selected(self.key, "安卓发行密钥")?;
                let password = self.secret()?;
                let package = self.selected(self.package, "安卓加密视频")?;
                let request = self.selected(self.request, "安卓设备请求")?;
                let (days, seconds) = self.period()?;
                ensure!(
                    package.with_extension("eav.adminmeta").is_file(),
                    "找不到对应 .eav.adminmeta，请与加密视频保存在同一目录"
                );
                let summary=format!("仅为安卓签发授权\r\n\r\n视频：{}\r\n设备请求：{}\r\n使用期限：首次成功激活起 {days} 天\r\n首次导入窗口：签发后 {seconds} 秒\r\n\r\n确认后选择 LIC 保存位置。",package.file_name().unwrap().to_string_lossy(),request.file_name().unwrap().to_string_lossy());
                if MessageBoxW(
                    self.window,
                    wide(&summary).as_ptr(),
                    wide("核对安卓授权").as_ptr(),
                    MB_OKCANCEL | MB_ICONINFORMATION,
                ) != IDOK
                {
                    return Ok(());
                }
                if let Some(p) = dialog(
                    self.window,
                    true,
                    "保存安卓授权文件",
                    "安卓授权文件\0*.lic\0",
                    "lic",
                )? {
                    ensure!(!p.exists(), "授权文件已存在，请使用新文件名");
                    self.start(
                        "正在校验设备请求并签发安卓 LIC…",
                        move || {
                            ensure!(
                                std::fs::metadata(&request)?.len() <= 256 * 1024,
                                "设备请求文件过大"
                            );
                            let request = serde_json::from_slice(&std::fs::read(request)?)?;
                            let lic = issue(
                                &Issuer::load(&k, &password)?,
                                &package,
                                &request,
                                days,
                                seconds,
                            )?;
                            use std::io::Write;
                            let mut out = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&p)?;
                            out.write_all(&serde_json::to_vec_pretty(&lic)?)?;
                            out.sync_all()?;
                            Ok(Done::License(p))
                        },
                    );
                }
            }
            HELP => {
                MessageBoxW(self.window,wide("1. 输入口令，选择或新建安卓发行密钥。\r\n2. 选择视频并生成 .eav，发送给安卓手机。\r\n3. 手机导入 .eav，导出设备 JSON 并传回电脑。\r\n4. 选择对应 .eav 和设备 JSON，设置期限并签发 LIC。\r\n5. 手机导入 LIC 后播放。\r\n\r\n这是独立的安卓授权工具，不签发 Windows 授权。\r\n发行密钥、口令和 .eav.adminmeta 请自行备份，勿发送给接收方。\r\n安卓播放器 APK 需单独安装，当前预览版只支持视频。").as_ptr(),wide("安卓视频授权流程").as_ptr(),MB_OK|MB_ICONINFORMATION);
            }
            _ => {}
        }
        Ok(())
    }
}
pub fn validate_period(days: &str, seconds: &str) -> Result<(u64, u64)> {
    let d: u64 = days.trim().parse().context("授权天数必须是正整数")?;
    let s: u64 = seconds
        .trim()
        .parse()
        .context("首次导入窗口必须是正整数秒数")?;
    ensure!((1..=36500).contains(&d), "授权天数须为 1–36500");
    ensure!(
        (1..=31536000).contains(&s),
        "首次导入窗口须为 1–31536000 秒"
    );
    Ok((d, s))
}
unsafe fn dialog(
    owner: HWND,
    save: bool,
    title: &str,
    filter: &str,
    ext: &str,
) -> Result<Option<PathBuf>> {
    let mut buffer = vec![0u16; 32768];
    let f = wide(filter);
    let t = wide(title);
    let e = wide(ext);
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = f.as_ptr();
    ofn.lpstrFile = buffer.as_mut_ptr();
    ofn.nMaxFile = buffer.len() as u32;
    ofn.lpstrTitle = t.as_ptr();
    ofn.lpstrDefExt = e.as_ptr();
    ofn.Flags = OFN_EXPLORER
        | OFN_PATHMUSTEXIST
        | OFN_NOCHANGEDIR
        | if save {
            OFN_OVERWRITEPROMPT
        } else {
            OFN_FILEMUSTEXIST
        };
    let ok = if save {
        GetSaveFileNameW(&mut ofn)
    } else {
        GetOpenFileNameW(&mut ofn)
    };
    if ok == 0 {
        let code = CommDlgExtendedError();
        if code != 0 {
            bail!("文件选择窗口失败：{code}");
        }
        return Ok(None);
    }
    use std::os::windows::ffi::OsStringExt;
    let n = buffer.iter().position(|&v| v == 0).unwrap_or(buffer.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..n],
    ))))
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let ui = &mut *ptr;
    match msg {
        WM_DRAWITEM => {
            ui.draw_item(&*(l as *const DRAWITEMSTRUCT));
            1
        }
        WM_PAINT => {
            let mut ps = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            ui.theme.copy(dc, 0, 0, 1152, 840);
            EndPaint(hwnd, &ps);
            0
        }
        WM_COMMAND => {
            if matches!(w & 0xffff, 201 | 202 | 203) {
                ui.period_summary();
                return 0;
            }
            if (w >> 16) == BN_CLICKED as usize {
                if let Err(e) = ui.action(w & 0xffff) {
                    ui.say(&format!("{e:#}"));
                    MessageBoxW(
                        hwnd,
                        wide(&format!("{e:#}")).as_ptr(),
                        wide("请检查输入").as_ptr(),
                        MB_OK | MB_ICONWARNING,
                    );
                }
            }
            0
        }
        WM_TIMER => {
            ui.poll();
            0
        }
        WM_CTLCOLORSTATIC => {
            SetBkMode(w as HDC, TRANSPARENT as i32);
            SetTextColor(w as HDC, 0x00573542);
            ui.background
        }
        WM_ERASEBKGND => {
            let mut r = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            ui.theme.copy(w as HDC, 0, 0, r.right, r.bottom);
            1
        }
        WM_CLOSE => {
            if ui.pending.is_some() {
                MessageBoxW(
                    hwnd,
                    wide("文件处理尚未完成，请稍候再关闭。").as_ptr(),
                    wide(TITLE).as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            } else {
                SetWindowTextW(ui.password, wide("").as_ptr());
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
pub fn error(s: &str) {
    unsafe {
        MessageBoxW(
            0,
            wide(s).as_ptr(),
            wide(TITLE).as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub fn run() -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let name = wide("EncryptAssetsAndroidAdministrator");
        let background = CreateSolidBrush(0x00fff7fa);
        let font = CreateFontW(
            -17,
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
            CLEARTYPE_QUALITY as u32,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        let title_font = CreateFontW(
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
            CLEARTYPE_QUALITY as u32,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = instance;
        wc.lpszClassName = name.as_ptr();
        wc.hbrBackground = background;
        wc.hCursor = LoadCursorW(0, IDC_ARROW);
        wc.hIcon = LoadIconW(instance, 1 as _);
        ensure!(RegisterClassW(&wc) != 0, "窗口注册失败");
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 1152,
            bottom: 840,
        };
        AdjustWindowRect(&mut rect, style, 0);
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            name.as_ptr(),
            wide(TITLE).as_ptr(),
            style,
            (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - height).max(0) / 2,
            width,
            height,
            0,
            0,
            instance,
            null(),
        );
        ensure!(hwnd != 0, "无法创建管理员窗口");
        let mut ui = Box::new(Ui {
            window: hwnd,
            key: 0,
            password: 0,
            video: 0,
            package: 0,
            request: 0,
            days: 0,
            seconds: 0,
            status: 0,
            controls: vec![],
            font,
            title_font,
            background,
            theme: Theme::new(1152, 840)?,
            unit: 0,
            summary: 0,
            english: false,
            translations: vec![],
            pending: None,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *ui as *mut Ui) as isize);
        ui.build();
        SetTimer(hwnd, 1, 100, None);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        DeleteObject(font);
        DeleteObject(title_font);
        DeleteObject(background);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn period_validation() {
        assert_eq!(validate_period("30", "3600").unwrap(), (30, 3600));
        for (d, s) in [
            ("0", "60"),
            ("-1", "60"),
            ("1.5", "60"),
            ("1", "0"),
            ("1", "31536001"),
            ("", "60"),
            ("999999999999999999999999", "60"),
        ] {
            assert!(validate_period(d, s).is_err());
        }
    }
}

fn english(s: &str) -> &str {
    match s {
"安卓 · 离线视频授权工作台"=>"Android · Offline video licensing workspace",
"先加密视频，再为安卓设备签发授权。首次导入窗口与使用期限独立设置。"=>"Encrypt video, then license the Android device. Import window and usage term are independent.",
"01  内容与发行密钥"=>"01  Content and issuer key",
"02  授权设置与签发"=>"02  License settings and issuance",
"新建发行密钥"=>"Create issuer key",
"选择已有密钥"=>"Select issuer key",
"密钥口令"=>"Passphrase",
"选择原视频"=>"Select original video",
"生成安卓加密视频 .eav"=>"Build encrypted Android video .eav",
"视频全屏循环 · 离线 LIC 验证\n密钥口令用于创建或解锁；管理员元数据请自行备份。"=>"Full-screen video loops · Offline LIC activation\nKeep your issuer key and administrator metadata backed up.",
"选择安卓加密视频 .eav"=>"Select encrypted Android video .eav",
"选择安卓设备识别文件 .json"=>"Select Android device request .json",
"授权天数"=>"License days",
"首次导入窗口"=>"First import window",
"核对并签发安卓 LIC"=>"Review and issue Android LIC",
"离线授权 · 你的专属播放空间"=>"Offline licensing · Your private playback space",
"本工具只处理安卓视频。Windows 授权请使用原管理员。\n发行密钥、内容包和 LIC 分开管理，不跨平台混用。"=>"Android video only. Use the original administrator for Windows.\nIssuer keys, content packages and LIC files are managed separately.",
"安卓使用说明"=>"Android help",
"请选择或新建安卓发行密钥。口令仅在本次进程中使用。"=>"Select or create an Android issuer key. Passphrases are not saved.",
_=>s}
}
