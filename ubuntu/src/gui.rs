use crate::{
    core::*,
    issuance::Options,
    platform::Device,
    session::{Authorization, Session},
};
use anyhow::{ensure, Result};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn tr(zh: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::current() == crate::i18n::Language::English {
        en
    } else {
        zh
    }
}
fn label(text: &str, class: &str) -> gtk::Label {
    let w = gtk::Label::new(Some(text));
    w.set_xalign(0.0);
    w.set_wrap(true);
    if !class.is_empty() {
        w.add_css_class(class);
    }
    w
}
fn button(text: &str, primary: bool) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    if primary {
        b.add_css_class("suggested-action");
    }
    b
}
fn vertical(space: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, space)
}
fn set_control_revealed(revealer: &gtk::Revealer, state: &Cell<bool>, reveal: bool) {
    if state.replace(reveal) == reveal {
        return;
    }
    revealer.set_reveal_child(reveal);
    #[cfg(feature = "watchdog-test-mode")]
    if let Some(path) = std::env::var_os("ENCRYPT_ASSETS_TEST_CONTROL_STATE_FILE") {
        let value = if reveal { "visible\n" } else { "hidden\n" };
        let _ = std::fs::write(path, value);
    }
}
fn message(win: &gtk::ApplicationWindow, text: &str) {
    let d = gtk::MessageDialog::builder()
        .transient_for(win)
        .modal(true)
        .message_type(gtk::MessageType::Info)
        .buttons(gtk::ButtonsType::Ok)
        .text(text)
        .build();
    d.connect_response(|d, _| d.close());
    d.present();
}
fn choose(win: &gtk::ApplicationWindow, save: bool, name: &str, done: impl Fn(PathBuf) + 'static) {
    let dialog = gtk::FileChooserNative::new(
        Some(if save {
            tr("保存文件", "Save file")
        } else {
            tr("选择文件", "Choose file")
        }),
        Some(win),
        if save {
            gtk::FileChooserAction::Save
        } else {
            gtk::FileChooserAction::Open
        },
        Some(if save {
            tr("保存", "Save")
        } else {
            tr("打开", "Open")
        }),
        Some(tr("取消", "Cancel")),
    );
    dialog.set_modal(true);
    if save {
        dialog.set_current_name(name);
    }
    dialog.connect_response(move |d, r| {
        if r == gtk::ResponseType::Accept {
            if let Some(p) = d.file().and_then(|f| f.path()) {
                done(p);
            }
        }
        d.destroy();
    });
    dialog.show();
}
fn job<T: Send + 'static>(
    win: &gtk::ApplicationWindow,
    status: &gtk::Label,
    body: &gtk::Box,
    work: impl FnOnce() -> Result<T> + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    body.set_sensitive(false);
    status.set_text(tr("正在处理，请稍候…", "Working, please wait…"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("后台任务异常")));
        let _ = tx.send(r);
    });
    let win = win.downgrade();
    let status = status.clone();
    let body = body.clone();
    let mut done = Some(done);
    glib::timeout_add_local(Duration::from_millis(80), move || {
        let Some(win) = win.upgrade() else {
            return glib::ControlFlow::Break;
        };
        match rx.try_recv() {
            Ok(result) => {
                body.set_sensitive(true);
                match result {
                    Ok(v) => {
                        status.set_text(tr("完成", "Complete"));
                        if let Some(f) = done.take() {
                            f(v);
                        }
                    }
                    Err(e) => {
                        status.set_text(&format!("{e:#}"));
                        message(&win, &format!("{e:#}"));
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                body.set_sensitive(true);
                status.set_text(tr("任务已中断", "Task interrupted"));
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        }
    });
}
fn background(body: &gtk::Box) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    let backdrop = gtk::Picture::new();
    if let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from_static(include_bytes!(
        "../assets/dragon-side.png"
    ))) {
        backdrop.set_paintable(Some(&t));
    }
    backdrop.set_content_fit(gtk::ContentFit::Cover);
    backdrop.set_can_shrink(true);
    backdrop.set_opacity(0.10);
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.set_can_target(false);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(body)
        .build();
    overlay.set_child(Some(&backdrop));
    overlay.add_overlay(&scroll);
    overlay
}
#[derive(Default)]
struct Admin {
    issuer: Option<Arc<Issuer>>,
    source: Option<PathBuf>,
    package: Option<PathBuf>,
    request: Option<PathBuf>,
    draft: Option<(String, String, u32)>,
}

fn administrator(win: &gtk::ApplicationWindow, state: Rc<RefCell<Admin>>) {
    let body = vertical(18);
    body.set_margin_top(26);
    body.set_margin_bottom(22);
    body.set_margin_start(28);
    body.set_margin_end(28);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = label("Encrypt your digital assets", "title");
    title.set_hexpand(true);
    header.append(&title);
    let language = button("中文 / English", false);
    header.append(&language);
    body.append(&header);
    body.append(&label(
        tr(
            "Ubuntu · 离线授权工作台",
            "Ubuntu · Offline licensing workspace",
        ),
        "subtitle",
    ));
    body.append(&label(tr("先加密打包，再为设备签发授权。首次导入窗口与使用期限分别设置。","Package encrypted content, then license a device. Import deadline and usage period are independent."),"muted"));
    let grid = gtk::Grid::builder()
        .column_spacing(22)
        .row_spacing(16)
        .column_homogeneous(true)
        .vexpand(true)
        .build();
    let left = vertical(12);
    left.add_css_class("card");
    let right = vertical(12);
    right.add_css_class("card");
    grid.attach(&left, 0, 0, 1, 1);
    grid.attach(&right, 1, 0, 1, 1);
    body.append(&grid);
    left.append(&label(
        tr("01  内容与发行密钥", "01  Content and issuer key"),
        "section",
    ));
    let key_state = label(
        if state.borrow().issuer.is_some() {
            tr("发行密钥已加载", "Issuer key loaded")
        } else {
            tr("请新建或导入发行密钥", "Create or import an issuer key")
        },
        "muted",
    );
    left.append(&key_state);
    let password = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .placeholder_text(tr(
            "密钥备份口令（至少 8 个字符）",
            "Backup password (at least 8 characters)",
        ))
        .build();
    left.append(&password);
    let keys = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let create = button(tr("新建并备份密钥", "Create & back up"), false);
    let import = button(tr("导入密钥", "Import key"), false);
    keys.append(&create);
    keys.append(&import);
    left.append(&keys);
    let source = label(
        &state
            .borrow()
            .source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| tr("尚未选择内容文件", "No content file selected").into()),
        "file-label",
    );
    left.append(&source);
    let open = button(
        tr(
            "选择视频 / 音频 / 文档 / 图片 / PPT",
            "Choose video / audio / document / image / PPT",
        ),
        false,
    );
    left.append(&open);
    let preview = button(tr("预览原文件", "Preview original"), false);
    left.append(&preview);
    let pack = button(tr("生成加密内容程序", "Build encrypted content app"), true);
    left.append(&pack);
    left.append(&label(tr("视频全屏循环 · 离线 LIC 验证\nPPT 保留原文件放映 · 不设置开机自启动","Full-screen looping video · Offline LIC validation\nOriginal presentation slideshows · No autostart"),"muted"));
    right.append(&label(
        tr("02  授权设置与签发", "02  License settings and issue"),
        "section",
    ));
    let package_label = label(
        &state
            .borrow()
            .package
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| tr("尚未选择内容程序", "No content app selected").into()),
        "file-label",
    );
    right.append(&package_label);
    let select_package = button(
        tr("选择已打包的内容程序", "Select packaged content app"),
        false,
    );
    right.append(&select_package);
    let request_label = label(
        &state
            .borrow()
            .request
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                tr(
                    "尚未导入设备识别文件",
                    "No encrypted device request selected",
                )
                .into()
            }),
        "file-label",
    );
    right.append(&request_label);
    let select_request = button(
        tr("导入设备识别文件 .json", "Import device request .json"),
        false,
    );
    right.append(&select_request);
    let settings = Options::load();
    let options_grid = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(10)
        .build();
    let days = gtk::Entry::builder()
        .text(settings.days.to_string())
        .input_purpose(gtk::InputPurpose::Digits)
        .width_chars(7)
        .build();
    let value = gtk::Entry::builder()
        .text(settings.value.to_string())
        .input_purpose(gtk::InputPurpose::Digits)
        .width_chars(7)
        .build();
    days.update_property(&[gtk::accessible::Property::Label(tr(
        "授权天数",
        "Usage days",
    ))]);
    value.update_property(&[gtk::accessible::Property::Label(tr(
        "首次导入窗口",
        "First import window",
    ))]);
    let unit = gtk::DropDown::from_strings(&[
        tr("秒", "seconds"),
        tr("分钟", "minutes"),
        tr("小时", "hours"),
        tr("天", "days"),
    ]);
    unit.set_selected(settings.unit as u32);
    if let Some((d, v, u)) = state.borrow().draft.as_ref() {
        days.set_text(d);
        value.set_text(v);
        unit.set_selected(*u);
    }
    options_grid.attach(&label(tr("授权天数", "Usage days"), ""), 0, 0, 1, 1);
    options_grid.attach(&days, 1, 0, 2, 1);
    options_grid.attach(
        &label(tr("首次导入窗口", "First import window"), ""),
        0,
        1,
        1,
        1,
    );
    options_grid.attach(&value, 1, 1, 1, 1);
    options_grid.attach(&unit, 2, 1, 1, 1);
    right.append(&options_grid);
    let summary = label(&settings.summary(), "muted");
    right.append(&summary);
    let issue = button(tr("确认并签发 LIC", "Confirm and issue LIC"), true);
    right.append(&issue);
    let status = label(
        tr(
            "就绪。密钥备份和 .olmeta 管理员记录请妥善保管。",
            "Ready. Keep the key backup and .olmeta administrator records safe.",
        ),
        "status",
    );
    body.append(&status);
    win.set_child(Some(&background(&body)));
    {
        let w = win.clone();
        let s = state.clone();
        language.connect_clicked(move |_| {
            let lang = if crate::i18n::current() == crate::i18n::Language::Chinese {
                crate::i18n::Language::English
            } else {
                crate::i18n::Language::Chinese
            };
            let _ = crate::i18n::select(lang);
            administrator(&w, s.clone());
        });
    }
    for creating in [true, false] {
        let btn = if creating {
            create.clone()
        } else {
            import.clone()
        };
        let w = win.clone();
        let s = state.clone();
        let password = password.clone();
        let status = status.clone();
        let body = body.clone();
        let key_state = key_state.clone();
        btn.connect_clicked(move |_| {
            let pw = zeroize::Zeroizing::new(password.text().to_string());
            if creating && pw.chars().count() < 8 {
                message(&w, tr("口令至少 8 个字符", "Use at least 8 characters"));
                return;
            }
            let (w2, s2, st, b, ks, pw_entry) = (
                w.clone(),
                s.clone(),
                status.clone(),
                body.clone(),
                key_state.clone(),
                password.clone(),
            );
            choose(&w, creating, "issuer.issuerkey", move |path| {
                let pw = pw.clone();
                let s = s2.clone();
                let ks = ks.clone();
                let pw_entry = pw_entry.clone();
                job(
                    &w2,
                    &st,
                    &b,
                    move || {
                        let key = if creating {
                            ensure!(!path.exists(), "备份文件已存在，请换一个名称");
                            let k = Issuer::create()?;
                            k.save(&path, &pw)?;
                            k
                        } else {
                            Issuer::load(&path, &pw)?
                        };
                        Ok(Arc::new(key))
                    },
                    move |key| {
                        s.borrow_mut().issuer = Some(key);
                        ks.set_text(tr("发行密钥已加载", "Issuer key loaded"));
                        pw_entry.set_text("");
                    },
                );
            });
        });
    }
    {
        let w = win.clone();
        let s = state.clone();
        let l = source.clone();
        open.connect_clicked(move |_| {
            let s = s.clone();
            let l = l.clone();
            let w2 = w.clone();
            choose(&w, false, "", move |p| match crate::packaging::kind(&p) {
                Ok(_) => {
                    l.set_text(&p.display().to_string());
                    s.borrow_mut().source = Some(p);
                }
                Err(e) => message(&w2, &e.to_string()),
            });
        });
    }
    {
        let w = win.clone();
        let s = state.clone();
        preview.connect_clicked(move |_| {
            if let Some(p) = s.borrow().source.as_ref() {
                if let Err(e) = gio::AppInfo::launch_default_for_uri(
                    &gio::File::for_path(p).uri(),
                    None::<&gio::AppLaunchContext>,
                ) {
                    message(&w, &e.to_string());
                }
            } else {
                message(&w, tr("请先选择原文件", "Select a source file first"));
            }
        });
    }
    {
        let (w, s, st, b, l) = (
            win.clone(),
            state.clone(),
            status.clone(),
            body.clone(),
            package_label.clone(),
        );
        pack.connect_clicked(move |_| {
            let (Some(issuer), Some(source)) =
                (s.borrow().issuer.clone(), s.borrow().source.clone())
            else {
                message(
                    &w,
                    tr(
                        "请先加载密钥并选择原文件",
                        "Load a key and select content first",
                    ),
                );
                return;
            };
            let (w2, s2, st, b, l) = (w.clone(), s.clone(), st.clone(), b.clone(), l.clone());
            let name = if std::env::var_os("ENCRYPT_ASSETS_BUNDLE").is_some() {
                "Content.run"
            } else if std::env::var_os("APPIMAGE").is_some() {
                "Content.AppImage"
            } else {
                "Content.bin"
            };
            choose(&w, true, name, move |p| {
                let (i, source, p2, s, l) = (
                    issuer.clone(),
                    source.clone(),
                    p.clone(),
                    s2.clone(),
                    l.clone(),
                );
                job(
                    &w2,
                    &st,
                    &b,
                    move || {
                        crate::packaging::pack(&source, &i, &p2)?;
                        Ok(p2)
                    },
                    move |p| {
                        l.set_text(&p.display().to_string());
                        s.borrow_mut().package = Some(p);
                    },
                );
            });
        });
    }
    {
        let (w, s, l) = (win.clone(), state.clone(), package_label.clone());
        select_package.connect_clicked(move |_| {
            let (w2, s, l) = (w.clone(), s.clone(), l.clone());
            choose(&w, false, "", move |p| match Package::open_exe(&p) {
                Ok(_) => {
                    l.set_text(&p.display().to_string());
                    s.borrow_mut().package = Some(p);
                }
                Err(e) => message(&w2, &e.to_string()),
            });
        });
    }
    {
        let (w, s, l) = (win.clone(), state.clone(), request_label.clone());
        select_request.connect_clicked(move |_| {
            let (w2, s, l) = (w.clone(), s.clone(), l.clone());
            choose(
                &w,
                false,
                "",
                move |p| match crate::hardware::read_enrollment(&p) {
                    Ok(_) => {
                        l.set_text(&p.display().to_string());
                        s.borrow_mut().request = Some(p);
                    }
                    Err(e) => message(&w2, &e.to_string()),
                },
            );
        });
    }
    let refresh: Rc<dyn Fn()> = Rc::new({
        let (d, v, u, summary, issue, s) = (
            days.clone(),
            value.clone(),
            unit.clone(),
            summary.clone(),
            issue.clone(),
            state.clone(),
        );
        move || {
            s.borrow_mut().draft = Some((d.text().to_string(), v.text().to_string(), u.selected()));
            match Options::parse(&d.text(), &v.text(), u.selected() as usize) {
                Ok(o) => {
                    summary.set_text(&o.summary());
                    issue.set_sensitive(true);
                }
                Err(e) => {
                    summary.set_text(&e.to_string());
                    issue.set_sensitive(false);
                }
            }
        }
    });
    refresh();
    {
        let f = refresh.clone();
        days.connect_changed(move |_| f());
    }
    {
        let f = refresh.clone();
        value.connect_changed(move |_| f());
    }
    {
        let f = refresh.clone();
        unit.connect_selected_notify(move |_| f());
    }
    {
        let (w, s, st, b) = (win.clone(), state.clone(), status.clone(), body.clone());
        issue.connect_clicked(move |_| {
            let (Some(issuer), Some(package), Some(request)) = (
                s.borrow().issuer.clone(),
                s.borrow().package.clone(),
                s.borrow().request.clone(),
            ) else {
                message(
                    &w,
                    tr(
                        "请加载密钥、内容程序和设备识别文件",
                        "Load an issuer key, content app and device request",
                    ),
                );
                return;
            };
            let options =
                match Options::parse(&days.text(), &value.text(), unit.selected() as usize) {
                    Ok(o) => o,
                    Err(e) => {
                        message(&w, &e.to_string());
                        return;
                    }
                };
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&w)
                .modal(true)
                .buttons(gtk::ButtonsType::OkCancel)
                .text(tr("确认签发本次授权？", "Issue this license? "))
                .secondary_text(options.summary())
                .build();
            let (w2, st, b) = (w.clone(), st.clone(), b.clone());
            dialog.connect_response(move |d, r| {
                d.close();
                if r != gtk::ResponseType::Ok {
                    return;
                }
                let (i, p, q, o, w3, st, b) = (
                    issuer.clone(),
                    package.clone(),
                    request.clone(),
                    options.clone(),
                    w2.clone(),
                    st.clone(),
                    b.clone(),
                );
                choose(&w2, true, "license.lic", move |out| {
                    let (i, p, q, o, out) =
                        (i.clone(), p.clone(), q.clone(), o.clone(), out.clone());
                    job(
                        &w3,
                        &st,
                        &b,
                        move || {
                            crate::packaging::issue_file(&i, &p, &q, &out, &o)?;
                            Ok(())
                        },
                        |_| {},
                    );
                });
            });
            dialog.present();
        });
    }
}

struct Recipient {
    path: PathBuf,
    info: PackageInfo,
    device: Option<Arc<Device>>,
    auth: Authorization,
    video: Option<crate::media::Video>,
    presentation: Option<crate::documents::Presentation>,
    serial: u64,
    last_media_time: i64,
    last_audio: i32,
    last_progress: Instant,
}
fn recipient(win: &gtk::ApplicationWindow, path: PathBuf, info: PackageInfo) {
    let body = vertical(20);
    body.set_margin_top(48);
    body.set_margin_bottom(38);
    body.set_margin_start(48);
    body.set_margin_end(48);
    body.append(&label("Encrypt your digital assets", "title"));
    body.append(&label(&info.display_name, "subtitle"));
    body.append(&label(tr("首次打开：导出本机设备识别文件，发送给管理员；收到授权后导入 LIC。","First use: export this device request, send it to your administrator, then import the returned LIC."),"muted"));
    let export = button(
        tr("获取本机设备识别码", "Export this device request"),
        false,
    );
    let import = button(
        tr("导入固态链密钥文件 / LIC", "Import license file / LIC"),
        true,
    );
    body.append(&export);
    body.append(&import);
    let status = label(
        tr("正在初始化设备授权…", "Initializing device authorization…"),
        "status",
    );
    body.append(&status);
    win.set_child(Some(&background(&body)));
    let state = Rc::new(RefCell::new(Recipient {
        path: path.clone(),
        info: info.clone(),
        device: None,
        auth: Arc::new(Mutex::new(Session::new(info.clone()))),
        video: None,
        presentation: None,
        serial: 0,
        last_media_time: -1,
        last_audio: 0,
        last_progress: Instant::now(),
    }));
    {
        let w = win.downgrade();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(w) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !w.is_visible() {
                return glib::ControlFlow::Break;
            }
            crate::watchdog::heartbeat();
            glib::ControlFlow::Continue
        });
    }
    {
        let (w, s, st, b) = (win.clone(), state.clone(), status.clone(), body.clone());
        job(
            win,
            &status,
            &body,
            move || Ok(Arc::new(Device::for_info(&info)?)),
            move |device| {
                s.borrow_mut().device = Some(device.clone());
                let auth = s.borrow().auth.clone();
                let s2 = s.clone();
                let w2 = w.clone();
                job(
                    &w,
                    &st,
                    &b,
                    move || {
                        auth.lock()
                            .map_err(|_| anyhow::anyhow!("授权锁异常"))?
                            .resume(&device)
                    },
                    move |key| {
                        if let Some(key) = key {
                            if let Err(e) = show_content(&w2, &s2, key) {
                                message(&w2, &e.to_string());
                            }
                        }
                    },
                );
            },
        );
    }
    {
        let (w, s, st, b) = (win.clone(), state.clone(), status.clone(), body.clone());
        export.connect_clicked(move |_| {
            let Some(device) = s.borrow().device.clone() else {
                return;
            };
            let info = s.borrow().info.clone();
            let (w2, st, b) = (w.clone(), st.clone(), b.clone());
            choose(&w, true, "device-request.json", move |p| {
                let (d, i) = (device.clone(), info.clone());
                job(
                    &w2,
                    &st,
                    &b,
                    move || atomic(&p, &encode(&crate::hardware::export(&i, &d)?)?),
                    |_| {},
                );
            });
        });
    }
    {
        let (w, s, st, b) = (win.clone(), state.clone(), status.clone(), body.clone());
        import.connect_clicked(move |_| {
            let Some(device) = s.borrow().device.clone() else {
                return;
            };
            let (auth, path) = (s.borrow().auth.clone(), s.borrow().path.clone());
            let (w2, s2, st, b) = (w.clone(), s.clone(), st.clone(), b.clone());
            choose(&w, false, "", move |p| {
                let (a, d, exe, s, w) = (
                    auth.clone(),
                    device.clone(),
                    path.clone(),
                    s2.clone(),
                    w2.clone(),
                );
                job(
                    &w2,
                    &st,
                    &b,
                    move || {
                        a.lock()
                            .map_err(|_| anyhow::anyhow!("授权锁异常"))?
                            .import(&p, &d, &exe)
                    },
                    move |key| {
                        if let Err(e) = show_content(&w, &s, key) {
                            message(&w, &e.to_string());
                        }
                    },
                );
            });
        });
    }
    {
        let s = state.clone();
        win.connect_close_request(move |_| {
            let mut s = s.borrow_mut();
            s.video.take();
            s.presentation.take();
            if let Ok(mut a) = s.auth.lock() {
                let _ = a.save();
            }
            glib::Propagation::Proceed
        });
    }
}
fn show_content(
    win: &gtk::ApplicationWindow,
    state: &Rc<RefCell<Recipient>>,
    key: Vec<u8>,
) -> Result<()> {
    let (path, info, auth) = {
        let s = state.borrow();
        (s.path.clone(), s.info.clone(), s.auth.clone())
    };
    let mut reader = Reader::new(Package::open_exe(&path)?, key)?;
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("viewer");
    let content = vertical(0);
    content.set_hexpand(true);
    content.set_vexpand(true);
    overlay.set_child(Some(&content));
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    controls.add_css_class("controls");
    controls.set_halign(gtk::Align::Center);
    let controls_revealed = Rc::new(Cell::new(true));
    let controls_revealer = gtk::Revealer::new();
    controls_revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    controls_revealer.set_transition_duration(240);
    controls_revealer.set_child(Some(&controls));
    let dock = vertical(0);
    dock.set_halign(gtk::Align::Center);
    dock.set_valign(gtk::Align::End);
    dock.set_margin_bottom(8);
    dock.append(&controls_revealer);
    let dock_edge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dock_edge.add_css_class("control-dock-edge");
    dock_edge.set_size_request(240, 7);
    dock.append(&dock_edge);
    let fullscreen = button(tr("全屏", "Full screen"), false);
    let exit = button(tr("关闭", "Close"), false);
    let progress = label("", "viewer-status");
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    match info.kind.as_str() {
        "media" => {
            let repeat = crate::packaging::is_video(&info.display_name);
            let video = crate::media::Video::open(reader, Some(auth.clone()), repeat)?;
            state.borrow_mut().video = Some(video);
            content.append(&picture);
            let play = button(tr("播放 / 暂停", "Play / pause"), false);
            controls.append(&play);
            let s = state.clone();
            play.connect_clicked(move |_| {
                if let Some(v) = s.borrow().video.as_ref() {
                    v.pause();
                }
            });
            let back = button("−10 s", false);
            let next = button("+10 s", false);
            for (b, delta) in [(&back, -10000), (&next, 10000)] {
                let s = state.clone();
                b.connect_clicked(move |_| {
                    if let Some(v) = s.borrow().video.as_ref() {
                        v.seek(v.time() + delta);
                    }
                });
                controls.append(b);
            }
            let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
            volume.set_value(80.0);
            volume.set_size_request(100, -1);
            volume.set_tooltip_text(Some(tr("音量", "Volume")));
            let s = state.clone();
            volume.connect_value_changed(move |v| {
                if let Some(player) = s.borrow().video.as_ref() {
                    player.volume(v.value() as i32);
                }
            });
            controls.append(&volume);
        }
        "image" => {
            let bytes = reader.all(256 * 1024 * 1024)?;
            let t = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes[..]))?;
            picture.set_paintable(Some(&t));
            content.append(&picture);
        }
        "text" => {
            let bytes = reader.all(32 * 1024 * 1024)?;
            let text = match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(_) => encoding_rs::GBK.decode(&bytes).0.into_owned(),
            };
            let view = gtk::TextView::builder()
                .editable(false)
                .cursor_visible(false)
                .wrap_mode(gtk::WrapMode::WordChar)
                .left_margin(24)
                .right_margin(24)
                .top_margin(24)
                .bottom_margin(80)
                .build();
            view.buffer().set_text(&text);
            let scroll = gtk::ScrolledWindow::builder()
                .hexpand(true)
                .vexpand(true)
                .child(&view)
                .build();
            content.append(&scroll);
        }
        "pdf" => {
            let pdf = Rc::new(crate::documents::Pdf::open(&mut reader)?);
            let page = Rc::new(Cell::new(0));
            let area = gtk::DrawingArea::new();
            area.set_hexpand(true);
            area.set_vexpand(true);
            let (p, n) = (pdf.clone(), page.clone());
            area.set_draw_func(move |_, ctx, w, h| p.draw(n.get(), ctx, w, h));
            content.append(&area);
            for (text, delta) in [(tr("上一页", "Previous"), -1), (tr("下一页", "Next"), 1)] {
                let b = button(text, false);
                let (n, p, a) = (page.clone(), pdf.clone(), area.clone());
                b.connect_clicked(move |_| {
                    n.set((n.get() + delta).clamp(0, p.pages() - 1));
                    a.queue_draw();
                });
                controls.append(&b);
            }
        }
        "presentation" => {
            state.borrow_mut().presentation =
                Some(crate::documents::Presentation::open(&mut reader, false)?);
            content.append(&label(tr("正在通过 LibreOffice Impress 放映…\n关闭本窗口将结束放映。","Presenting through LibreOffice Impress…\nClose this window to end the slideshow."),"subtitle"));
        }
        _ => anyhow::bail!("不支持的内容类型"),
    }
    controls.append(&progress);
    controls.append(&fullscreen);
    controls.append(&exit);
    overlay.add_overlay(&dock);
    win.set_child(Some(&overlay));
    let full = Rc::new(Cell::new(false));
    {
        let (w, f) = (win.clone(), full.clone());
        fullscreen.connect_clicked(move |_| {
            f.set(!f.get());
            w.set_decorated(!f.get());
            if f.get() {
                w.fullscreen();
            } else {
                w.unfullscreen();
            }
        });
    }
    {
        let w = win.clone();
        exit.connect_clicked(move |_| w.close());
    }
    let motion_time = Rc::new(Cell::new(Instant::now()));
    let motion = gtk::EventControllerMotion::new();
    {
        let (c, v, t) = (
            controls_revealer.clone(),
            controls_revealed.clone(),
            motion_time.clone(),
        );
        motion.connect_motion(move |_, _, _| {
            set_control_revealed(&c, &v, true);
            t.set(Instant::now());
        });
    }
    overlay.add_controller(motion);
    let keyboard = gtk::EventControllerKey::new();
    {
        let (w, s, f, c, v) = (
            win.clone(),
            state.clone(),
            full.clone(),
            controls_revealer.clone(),
            controls_revealed.clone(),
        );
        keyboard.connect_key_pressed(move |_, key, _, _| {
            match key {
                gdk::Key::Escape => {
                    f.set(false);
                    w.unfullscreen();
                    w.set_decorated(true);
                    set_control_revealed(&c, &v, true);
                }
                gdk::Key::F11 => {
                    f.set(!f.get());
                    w.set_decorated(!f.get());
                    if f.get() {
                        w.fullscreen();
                    } else {
                        w.unfullscreen();
                    }
                }
                gdk::Key::space => {
                    if let Some(v) = s.borrow().video.as_ref() {
                        v.pause();
                    }
                }
                gdk::Key::Left | gdk::Key::Right => {
                    if let Some(v) = s.borrow().video.as_ref() {
                        v.seek(v.time() + if key == gdk::Key::Left { -10000 } else { 10000 });
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    win.add_controller(keyboard);
    if info.kind == "media" && crate::packaging::is_video(&info.display_name) {
        full.set(true);
        win.set_decorated(false);
        win.fullscreen();
        set_control_revealed(&controls_revealer, &controls_revealed, false);
    }
    let (w, s, f, c, v) = (
        win.downgrade(),
        state.clone(),
        full.clone(),
        controls_revealer.clone(),
        controls_revealed.clone(),
    );
    let started = Instant::now();
    glib::timeout_add_local(Duration::from_millis(33), move || {
        let Some(w) = w.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !w.is_visible() {
            return glib::ControlFlow::Break;
        }
        let mut s = s.borrow_mut();
        let mut presentation_ended = false;
        let result = (|| -> Result<()> {
            {
                let mut a = s.auth.lock().map_err(|_| anyhow::anyhow!("授权锁异常"))?;
                a.validate()?;
                if now() - a.last_saved >= 5 {
                    a.save()?;
                }
            }
            let old_serial = s.serial;
            if let Some(v) = s.video.as_mut() {
                v.tick()?;
                let serial = if let Ok(frame) = v.frame.try_lock() {
                    if frame.serial != old_serial && !frame.pixels.is_empty() {
                        let t = gdk::MemoryTexture::new(
                            frame.width as i32,
                            frame.height as i32,
                            gdk::MemoryFormat::B8g8r8a8,
                            &glib::Bytes::from(frame.bytes()),
                            frame.stride as usize,
                        );
                        picture.set_paintable(Some(&t));
                    }
                    frame.serial
                } else {
                    old_serial
                };
                let (time, state, audio) = (v.time(), v.state(), v.audio_progress());
                progress.set_text(&format!(
                    "{} / {}",
                    clock_text(time),
                    clock_text(v.length())
                ));
                if state == 4
                    || state == 6
                    || time != s.last_media_time
                    || serial != s.serial
                    || audio != s.last_audio
                {
                    s.last_progress = Instant::now();
                }
                s.last_media_time = time;
                s.last_audio = audio;
                s.serial = serial;
                ensure!(
                    started.elapsed() < Duration::from_secs(90)
                        || s.last_progress.elapsed() < Duration::from_secs(45),
                    "媒体播放无进展"
                );
            }
            if let Some(p) = s.presentation.as_mut() {
                if !p.poll()? {
                    s.presentation.take();
                    presentation_ended = true;
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            // Expired/invalid authorization is a normal stop, never a watchdog fault.
            let auth_failed = s
                .auth
                .lock()
                .map(|mut a| a.validate().is_err())
                .unwrap_or(true);
            s.video.take();
            s.presentation.take();
            let msg = format!("{e:#}");
            drop(s);
            if auth_failed {
                w.unfullscreen();
                w.set_decorated(true);
                w.set_child(Some(&label(&msg, "status")));
                message(&w, &msg);
                return glib::ControlFlow::Break;
            }
            eprintln!("PLAYBACK_FAULT: {msg}");
            std::process::exit(75);
        }
        if presentation_ended {
            drop(s);
            w.close();
            return glib::ControlFlow::Break;
        }
        if f.get() && motion_time.get().elapsed() > Duration::from_secs(3) {
            set_control_revealed(&c, &v, false);
        }
        glib::ControlFlow::Continue
    });
    Ok(())
}
fn clock_text(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn run(content: Option<(PathBuf, PackageInfo)>) -> Result<()> {
    // Materialize the embedded icon for both standalone content and installed builds.
    // This is only an icon cache; no login or boot startup entries are created.
    let icon_root = crate::platform::root().join("icons");
    let icon_dir = icon_root.join("hicolor/512x512/apps");
    std::fs::create_dir_all(&icon_dir)?;
    std::fs::write(icon_root.join("hicolor/index.theme"),
        "[Icon Theme]\nName=Hicolor\nComment=Encrypt assets embedded fallback\nDirectories=512x512/apps\n\n[512x512/apps]\nSize=512\nContext=Applications\nType=Fixed\n")?;
    std::fs::write(
        icon_dir.join("io.github.shuizhaodemumu.EncryptAssetsUbuntu.png"),
        include_bytes!("../assets/app-icon.png"),
    )?;
    let app = gtk::Application::builder()
        .application_id("io.github.shuizhaodemumu.EncryptAssetsUbuntu")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        gtk::Window::set_default_icon_name("io.github.shuizhaodemumu.EncryptAssetsUbuntu");
        let css = gtk::CssProvider::new();
        css.load_from_data(include_str!("../assets/style.css"));
        if let Some(display) = gdk::Display::default() {
            gtk::IconTheme::for_display(&display).add_search_path(&icon_root);
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .icon_name("io.github.shuizhaodemumu.EncryptAssetsUbuntu")
            .title("Encrypt your digital assets · Ubuntu")
            .default_width(1120)
            .default_height(840)
            .build();
        match content.clone() {
            Some((p, i)) => recipient(&win, p, i),
            None => administrator(&win, Rc::new(RefCell::new(Admin::default()))),
        }
        win.present();
        #[cfg(feature = "watchdog-test-mode")]
        if let Ok(s) = std::env::var("ENCRYPT_ASSETS_TEST_CLOSE_SECONDS") {
            if let Ok(seconds) = s.parse::<u64>() {
                let w = win.downgrade();
                glib::timeout_add_local_once(Duration::from_secs(seconds), move || {
                    if let Some(w) = w.upgrade() {
                        w.close();
                    }
                });
            }
        }
    });
    app.run_with_args::<&str>(&[]);
    Ok(())
}
