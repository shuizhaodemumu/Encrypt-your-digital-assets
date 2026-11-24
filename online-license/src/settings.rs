use crate::*;
use windows_sys::Win32::Graphics::Gdi::*;
const LAVENDER: u32 = 0x00fbf1f6;
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::EnableWindow, WindowsAndMessaging::*},
};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn get(h: HWND) -> String {
    let mut b = vec![0; GetWindowTextLengthW(h) as usize + 1];
    GetWindowTextW(h, b.as_mut_ptr(), b.len() as i32);
    String::from_utf16_lossy(&b[..b.len() - 1])
}
struct Ui {
    config: AdminConfig,
    done: bool,
    saved: bool,
    groups: Value,
    key: String,
    rx: Option<std::sync::mpsc::Receiver<Result<Value>>>,
}
unsafe fn status(h: HWND, s: &str) {
    SetWindowTextW(GetDlgItem(h, 8), wide(s).as_ptr());
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_NCCREATE {
        let c = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(h, GWLP_USERDATA, c.lpCreateParams as isize);
        return DefWindowProcW(h, m, w, l);
    }
    if m == WM_ERASEBKGND || m == WM_CTLCOLORSTATIC {
        let dc = w as HDC;
        SetDCBrushColor(dc, LAVENDER);
        let brush = GetStockObject(DC_BRUSH) as HBRUSH;
        if m == WM_CTLCOLORSTATIC {
            SetBkColor(dc, LAVENDER);
            SetTextColor(dc, 0x006c3654);
            return brush as isize;
        }
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(h, &mut rect);
        FillRect(dc, &rect, brush);
        let bmp = include_bytes!("dragon.bmp");
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = std::ptr::read_unaligned(bmp.as_ptr().add(14) as *const BITMAPINFOHEADER);
        let offset = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
        StretchDIBits(
            dc,
            528,
            78,
            190,
            230,
            0,
            0,
            190,
            230,
            bmp.as_ptr().add(offset).cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        return 1;
    }
    if !matches!(m, WM_COMMAND | WM_TIMER | WM_CLOSE) {
        return DefWindowProcW(h, m, w, l);
    }
    if m == WM_COMMAND && !matches!(w & 65535, 3 | 7) {
        return 0;
    }
    let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut Ui;
    if p.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    let u = &mut *p;
    match m {
        WM_COMMAND => {
            let id = w & 65535;
            if id == 3 {
                if u.rx.is_some() {
                    return 0;
                }
                let url = get(GetDlgItem(h, 1)).trim_end_matches('/').to_string();
                let token = get(GetDlgItem(h, 2));

                let cert_path = get(GetDlgItem(h, 12));
                let ca = if cert_path.trim() == "[已保存证书]" {
                    u.config.server_ca.clone()
                } else if cert_path.trim().is_empty() {
                    None
                } else {
                    match std::fs::read_to_string(cert_path.trim()) {
                        Ok(pem) => Some(pem),
                        Err(e) => {
                            status(h, &format!("读取证书失败：{e}"));
                            return 0;
                        }
                    }
                };
                u.config.server_ca = ca.clone();
                u.config.url = url.clone();
                u.config.token = token.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                u.rx = Some(rx);
                status(h, "正在连接，请稍候…");
                EnableWindow(GetDlgItem(h, 7), 0);
                std::thread::spawn(move || {
                    let r = admin_with_ca(&url, &token, None, ca.as_deref());
                    let _ = tx.send(r);
                });
            }
            if id == 7 && u.rx.is_none() {
                let index = SendMessageW(GetDlgItem(h, 6), CB_GETCURSEL, 0, 0);
                if index < 0 {
                    status(h, "请连接服务器并选择已有组");
                    return 0;
                }
                if let Some(g) = u.groups.as_array().and_then(|a| a.get(index as usize)) {
                    let policy = Policy {
                        server_ca: u.config.server_ca.clone(),
                        server_url: u.config.url.clone(),
                        server_key: u.key.clone(),
                        group_id: g["id"].as_str().unwrap_or("").into(),
                    };
                    if policy.validate().is_ok() {
                        u.config.policy = Some(policy);
                        u.config.group_name = g["name"].as_str().unwrap_or("").into();
                        u.config.enabled = true;
                        u.saved = true;
                        u.done = true;
                        DestroyWindow(h);
                    }
                }
            }
            0
        }
        WM_TIMER => {
            if let Some(rx) = &u.rx {
                if let Ok(result) = rx.try_recv() {
                    u.rx = None;
                    match result {
                        Ok(v) => {
                            u.key = v["server_key"].as_str().unwrap_or("").into();
                            u.groups = Value::Array(
                                v["groups"]
                                    .as_array()
                                    .unwrap_or(&vec![])
                                    .iter()
                                    .filter(|g| g["enabled"] == true)
                                    .cloned()
                                    .collect(),
                            );
                            let combo = GetDlgItem(h, 6);
                            SendMessageW(combo, CB_RESETCONTENT, 0, 0);
                            for g in u.groups.as_array().unwrap() {
                                SendMessageW(
                                    combo,
                                    CB_ADDSTRING,
                                    0,
                                    wide(g["name"].as_str().unwrap_or("")).as_ptr() as isize,
                                );
                            }
                            SendMessageW(combo, CB_SETCURSEL, 0, 0);
                            let available = !u.groups.as_array().unwrap().is_empty();
                            EnableWindow(GetDlgItem(h, 7), available as i32);
                            status(
                                h,
                                if available {
                                    "已连接，请选择授权组并保存。"
                                } else {
                                    "没有可用组，请到 NAS 在线平台创建后刷新。"
                                },
                            );
                        }
                        Err(e) => status(h, &format!("连接失败：{e}")),
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            u.done = true;
            DestroyWindow(h);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
pub fn edit(owner: HWND, config: AdminConfig) -> Result<Option<AdminConfig>> {
    unsafe {
        let class = wide("OnlineLicenseSettings");
        let instance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(procedure);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hbrBackground = 0;
        wc.hCursor = LoadCursorW(0, IDC_ARROW);
        RegisterClassW(&wc);
        let mut ui = Box::new(Ui {
            config,
            done: false,
            saved: false,
            groups: json!([]),
            key: String::new(),
            rx: None,
        });
        let h = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            wide("在线监管 · 连接与组管理").as_ptr(),
            WS_CLIPCHILDREN | WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            460,
            owner,
            0,
            instance,
            &mut *ui as *mut Ui as *mut _,
        );
        ensure!(h != 0, "无法打开在线设置");
        let control = |class: &str,
                       text: &str,
                       id: usize,
                       x: i32,
                       y: i32,
                       w: i32,
                       height: i32,
                       style: u32| {
            CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                w,
                height,
                h,
                id as isize,
                instance,
                std::ptr::null(),
            )
        };
        let edit = WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP;
        control("STATIC", "在线授权 · 连接管理器", 14, 24, 18, 470, 26, 0);
        control("STATIC", "HTTPS 服务器地址", 10, 24, 57, 475, 22, 0);
        control(
            "EDIT",
            if ui.config.url.trim().is_empty() {
                option_env!("LICENSE_MANAGER_DEFAULT_URL").unwrap_or("")
            } else {
                &ui.config.url
            },
            1,
            24,
            80,
            484,
            30,
            edit,
        );
        control("STATIC", "管理员令牌", 11, 24, 119, 470, 22, 0);
        control(
            "EDIT",
            &ui.config.token,
            2,
            24,
            142,
            484,
            30,
            edit | ES_PASSWORD as u32,
        );
        control(
            "STATIC",
            "私有 CA 证书路径（公共 HTTPS 可留空）",
            13,
            24,
            181,
            480,
            22,
            0,
        );
        control(
            "EDIT",
            if ui.config.server_ca.is_some() {
                "[已保存证书]"
            } else {
                ""
            },
            12,
            24,
            204,
            484,
            30,
            edit,
        );
        control("STATIC", "授权组", 15, 24, 246, 480, 22, 0);
        control(
            "COMBOBOX",
            "",
            6,
            24,
            269,
            320,
            155,
            CBS_DROPDOWNLIST as u32 | WS_VSCROLL | WS_TABSTOP,
        );
        control("BUTTON", "连接 / 刷新组", 3, 354, 266, 154, 32, WS_TABSTOP);
        control("BUTTON", "选中此组并保存", 7, 24, 316, 212, 36, WS_TABSTOP);
        EnableWindow(GetDlgItem(h, 7), 0);
        control(
            "STATIC",
            "请在 NAS 在线平台创建组，然后连接并刷新列表。",
            8,
            24,
            367,
            680,
            34,
            0,
        );
        let font = CreateFontW(
            -16,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        for id in 1..=15 {
            let child = GetDlgItem(h, id);
            if child != 0 {
                SendMessageW(child, WM_SETFONT, font as usize, 1);
            }
        }
        EnableWindow(owner, 0);
        SetTimer(h, 1, 100, None);
        ShowWindow(h, SW_SHOW);
        let mut msg: MSG = std::mem::zeroed();
        while !ui.done && GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if IsDialogMessageW(h, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        EnableWindow(owner, 1);
        SetForegroundWindow(owner);
        DeleteObject(font);
        Ok(if ui.saved {
            Some(ui.config.clone())
        } else {
            None
        })
    }
}
pub fn load_config(namespace: &str) -> Result<AdminConfig> {
    let p = config_path(namespace);
    if !p.exists() {
        return Ok(AdminConfig::default());
    }
    Ok(serde_json::from_slice(&protect(
        &std::fs::read(p)?,
        false,
    )?)?)
}
pub fn save_config(namespace: &str, c: &AdminConfig) -> Result<()> {
    let p = config_path(namespace);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, protect(&serde_json::to_vec(c)?, true)?)?;
    Ok(())
}
fn config_path(namespace: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join(namespace)
        .join("online-admin.bin")
}
fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::*;
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output: CRYPT_INTEGER_BLOB = std::mem::zeroed();
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        ensure!(ok != 0, "本地连接配置无法解密");
        let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = bytes.to_vec();
        bytes.fill(0);
        LocalFree(output.pbData as *mut _);
        Ok(result)
    }
}
