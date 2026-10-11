#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod automation;
mod core;
mod gui;
mod hardware;
mod i18n;
mod install;
mod native;
mod platform;
mod presentation;
mod session;
mod watchdog;
use anyhow::{ensure, Result};
use core::*;
use std::{
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
};
fn main() {
    i18n::init();
    if let Err(e) = run() {
        eprintln!("{e:#}");
        if std::env::args().len() == 1 {
            gui::message(&format!("{e:#}"));
        }
        std::process::exit(1)
    }
}
fn password() -> Result<String> {
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim_end_matches(['\r', '\n']).into())
}
fn upgrade(source: &Path, output: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    ensure!(
        source != output && output != exe,
        crate::i18n::text("请保留原文件并保存为新 EXE")
    );
    let mut p = Package::open_exe(source)?;
    ensure!(
        p.info.version == 2,
        crate::i18n::text("旧版内容需要在新需求版管理员中重新打包，不能只替换查看器")
    );
    let expected_hash = p.encrypted_hash()?;
    let temp = output.with_extension(format!("{}.pkg", uuid::Uuid::new_v4()));
    p.file.seek(std::io::SeekFrom::Start(p.base))?;
    let mut f = fs::File::create(&temp)?;
    std::io::copy(&mut p.file.take(p.length), &mut f)?;
    drop(f);
    let result = build_exe(&exe, &temp, output);
    let _ = fs::remove_file(temp);
    result?;
    ensure!(
        Package::open_exe(output)?.encrypted_hash()? == expected_hash,
        crate::i18n::text("迁移后的密文不一致")
    );
    let meta = PathBuf::from(format!("{}.olmeta", source.display()));
    if meta.exists() {
        fs::copy(meta, format!("{}.olmeta", output.display()))?;
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let exe = std::env::current_exe()?;
    let info = Package::open_exe(&exe).ok().map(|p| p.info);
    if args.len() > 1 {
        let expected = match args[1].as_str() {
            "--pack" => Some(5),
            "--issue" => Some(7),
            "--decrypt-check" | "--upgrade" | "--watchdog" | "--enroll" => Some(4),
            "--export-device" => Some(3),
            "--ppt-host" => Some(3),
            _ => None,
        };
        if let Some(expected) = expected {
            ensure!(
                args.len() == expected,
                crate::i18n::text("命令参数数量不正确")
            );
        }
        match args[1].as_str() {
            "--ppt-host" => {
                presentation::host(Path::new(&args[2]))?;
                return Ok(());
            }
            "--powerpoint-check" => {
                println!("{}", presentation::available());
                return Ok(());
            }
            "--watchdog" => {
                let info = info
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("运行监测仅支持内容 EXE")))?;
                ensure!(args.len() == 4, crate::i18n::text("监测参数无效"));
                watchdog::supervise(&exe, info, args[2].parse()?, &args[3])?;
                return Ok(());
            }
            "--enroll" => {
                ensure!(
                    args.len() == 4,
                    crate::i18n::text("用法：--enroll 内容EXE 输出.json")
                );
                let info = Package::open_exe(Path::new(&args[2]))?.info;
                let device = platform::Device::for_info(&info)?;
                let encrypted = hardware::export(&info, &device)?;
                atomic(Path::new(&args[3]), &encode(&encrypted)?)?;
                return Ok(());
            }
            "--disable-startup" => {
                let info = if let Some(info) = info.clone() {
                    info
                } else {
                    Package::open_exe(Path::new(
                        args.get(2)
                            .ok_or_else(|| anyhow::anyhow!(crate::i18n::text("请选择内容EXE")))?,
                    ))?
                    .info
                };
                watchdog::disable_startup(&info)?;
                return Ok(());
            }
            "--export-device" => {
                let d = platform::Device::load()?;
                atomic(Path::new(&args[2]), &encode(&d.request()?)?)?;
                println!("{}", crate::i18n::text("设备请求已导出"));
                return Ok(());
            }
            "--inspect" => {
                let p = if args.len() == 4 {
                    Package::open_exe(Path::new(&args[2]))?
                } else {
                    Package::open_exe(&exe)?
                };
                let out = &args[args.len() - 1];
                atomic(Path::new(out), &encode(&p.info)?)?;
                return Ok(());
            }
            "--upgrade" => {
                upgrade(Path::new(&args[2]), Path::new(&args[3]))?;
                return Ok(());
            }
            "--pack" => {
                let source = Path::new(&args[2]);
                let issuer = Issuer::load(Path::new(&args[3]), &password()?)?;
                let out = Path::new(&args[4]);
                let kind = match source
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!(crate::i18n::text("输入文件必须有受支持的扩展名"))
                    })?
                    .to_lowercase()
                    .as_str()
                {
                    "mp4" | "mp3" | "wav" => "media",
                    "pdf" => "pdf",
                    "txt" => "text",
                    "png" | "jpg" => "image",
                    "ppt" | "pptx" | "pps" | "ppsx" => "presentation",
                    _ => anyhow::bail!(crate::i18n::text("CLI 不支持该格式，请使用界面转换")),
                };
                if kind == "presentation" {
                    presentation::validate(source)?;
                }
                let pubkey = issuer.spki()?;
                let info = PackageInfo {
                    version: 2,
                    content_id: uuid::Uuid::new_v4(),
                    issuer_fingerprint: fingerprint(&pubkey),
                    issuer_public_key: b64(&pubkey),
                    display_name: source.file_name().unwrap().to_string_lossy().into(),
                    kind: kind.into(),
                    plain_length: fs::metadata(source)?.len(),
                    chunk_size: CHUNK,
                    nonce_prefix: b64(&random::<8>()),
                    hardware_policy: crate::hardware::STABLE.into(),
                    enrollment_public_key: issuer.enrollment_public()?,
                };
                let key = random::<32>();
                let temp = out.with_extension("pkg");
                write_package(source, &temp, &info, &key)?;
                build_exe(&exe, &temp, out)?;
                issuer.catalog_save(
                    &PathBuf::from(format!("{}.olmeta", out.display())),
                    &Catalog {
                        info,
                        key_base64: b64(&key),
                    },
                )?;
                fs::remove_file(temp)?;
                return Ok(());
            }
            "--issue" => {
                let issuer = Issuer::load(Path::new(&args[2]), &password()?)?;
                let c = issuer.catalog_load(Path::new(&args[3]))?;
                ensure!(
                    c.info.version == 2,
                    crate::i18n::text("请选择新需求版生成的内容包")
                );
                let request = hardware::request_for(&issuer, &c.info, Path::new(&args[4]))?;
                let days = args[5].parse()?;
                let signed = issue(&issuer, &c, &request, days, now())?;
                atomic(Path::new(&args[6]), &encode(&signed)?)?;
                return Ok(());
            }
            "--decrypt-check" => {
                let p = Package::open_exe(Path::new(&args[2]))?;
                let issuer = Issuer::load(Path::new(&args[3]), &password()?)?;
                let c = issuer.catalog_load(Path::new(&format!("{}.olmeta", args[2])))?;
                ensure!(c.info == p.info, crate::i18n::text("内容记录不一致"));
                let mut reader = Reader::new(p, unb64(&c.key_base64)?)?;
                let mut digest = sha2::Sha256::new();
                use sha2::Digest;
                let mut buf = vec![0; CHUNK];
                loop {
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    digest.update(&buf[..n]);
                }
                println!(
                    "{}",
                    digest
                        .finalize()
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<String>()
                );
                return Ok(());
            }
            "--install" => {
                install::install(&exe, info.as_ref())?;
                return Ok(());
            }
            "--uninstall" => {
                let remove = if args.iter().any(|s| s == "--silent") {
                    args.iter().any(|s| s == "--remove-state")
                } else {
                    unsafe {
                        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                            0,
                            platform::wide(crate::i18n::text("是否同时删除本地授权数据？"))
                                .as_ptr(),
                            platform::wide(crate::i18n::text("卸载 Rust 版")).as_ptr(),
                            windows_sys::Win32::UI::WindowsAndMessaging::MB_YESNO,
                        ) == windows_sys::Win32::UI::WindowsAndMessaging::IDYES
                    }
                };
                install::uninstall(info.as_ref(), remove)?;
                return Ok(());
            }
            _ => {}
        }
    }
    let _instance = if let Some(info) = info.as_ref() {
        ensure!(
            info.version == 2,
            crate::i18n::text("旧版内容请用原版程序打开，或重新打包为完整硬件绑定版本")
        );
        match platform::viewer_instance(info)? {
            Some(instance) => Some(instance),
            None => return Ok(()),
        }
    } else {
        None
    };
    let role = if let Some(info) = info {
        gui::Role::Viewer(info)
    } else if args.iter().any(|s| s == "--device")
        || exe
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case("DeviceTool")
    {
        gui::Role::Device
    } else if exe
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .eq_ignore_ascii_case("Packager-Setup")
    {
        gui::Role::Setup
    } else {
        gui::Role::Administrator
    };
    gui::App::run(role, exe)
}
