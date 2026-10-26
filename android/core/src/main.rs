#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#[cfg(windows)]
mod admin_theme;
#[cfg(windows)]
mod admin_ui;
use anyhow::{ensure, Result};
use encrypt_video::{issue, pack, Issuer};
use std::path::Path;
fn main() {
    #[cfg(windows)]
    if std::env::args_os().len() == 1 {
        if let Err(e) = admin_ui::run() {
            admin_ui::error(&format!("{e:#}"));
        }
        return;
    }
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    if let Err(e) = cli() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
fn cli() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1)
        .map(|s| s == "--help" || s == "-h")
        .unwrap_or(false)
    {
        println!("Android video administrator: double-click for GUI. CLI: init KEY | pack KEY VIDEO OUTPUT.eav | issue KEY PACKAGE REQUEST OUTPUT.lic DAYS WINDOW_SECONDS. Set EAV_ISSUER_PASSWORD for CLI only.");
        return Ok(());
    }
    ensure!(a.len()>=3,"用法: video-admin init KEY | pack KEY VIDEO OUTPUT.eav | issue KEY PACKAGE.eav REQUEST.json OUTPUT.lic DAYS WINDOW_SECONDS；通过 EAV_ISSUER_PASSWORD 设置密钥口令");
    let password = std::env::var("EAV_ISSUER_PASSWORD")
        .map_err(|_| anyhow::anyhow!("请设置发行密钥口令 EAV_ISSUER_PASSWORD"))?;
    match a[1].as_str() {
        "init" => {
            ensure!(!Path::new(&a[2]).exists(), "密钥文件已存在");
            Issuer::create()?.save(Path::new(&a[2]), &password)?;
        }
        "pack" => {
            ensure!(a.len() == 5, "pack KEY VIDEO OUTPUT.eav");
            pack(
                &Issuer::load(Path::new(&a[2]), &password)?,
                Path::new(&a[3]),
                Path::new(&a[4]),
            )?;
        }
        "issue" => {
            ensure!(
                a.len() == 8,
                "issue KEY PACKAGE REQUEST OUTPUT DAYS WINDOW_SECONDS"
            );
            ensure!(!Path::new(&a[5]).exists(), "输出授权文件已存在");
            let request = serde_json::from_slice(&std::fs::read(&a[4])?)?;
            let lic = issue(
                &Issuer::load(Path::new(&a[2]), &password)?,
                Path::new(&a[3]),
                &request,
                a[6].parse()?,
                a[7].parse()?,
            )?;
            std::fs::write(&a[5], serde_json::to_vec_pretty(&lic)?)?;
        }
        _ => anyhow::bail!("未知命令"),
    }
    println!("完成");
    Ok(())
}
