mod core;
mod documents;
mod gui;
mod hardware;
mod i18n;
mod issuance;
mod linux_hardware;
mod media;
mod packaging;
mod platform;
mod session;
mod verification;
mod watchdog;
use anyhow::{ensure, Result};
use core::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub fn content_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ENCRYPT_ASSETS_BUNDLE").map(PathBuf::from) {
        if p.is_file() && Package::open_exe(&p).is_ok() {
            return Ok(p);
        }
    }
    if let Some(p) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        if p.is_file() && Package::open_exe(&p).is_ok() {
            return Ok(p);
        }
    }
    Ok(std::env::var_os("ENCRYPT_ASSETS_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?))
}
pub fn template_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ENCRYPT_ASSETS_BUNDLE").map(PathBuf::from) {
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Some(p) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        if p.is_file() {
            return Ok(p);
        }
    }
    Ok(std::env::current_exe()?)
}
fn password() -> Result<zeroize::Zeroizing<String>> {
    let mut p = String::new();
    std::io::stdin().read_line(&mut p)?;
    Ok(zeroize::Zeroizing::new(
        p.trim_end_matches(['\r', '\n']).to_string(),
    ))
}
fn run() -> Result<()> {
    i18n::init();
    let a: Vec<String> = std::env::args().collect();
    let exe = content_path()?;
    let packaged = Package::open_exe(&exe).ok().map(|p| p.info);
    match a.get(1).map(String::as_str){
        Some("--version")=>println!("Encrypt your digital assets Ubuntu {}",env!("CARGO_PKG_VERSION")),
        Some("--create-issuer")=>{ensure!(a.len()==3,"--create-issuer KEY");ensure!(!Path::new(&a[2]).exists(),"密钥备份已存在");Issuer::create()?.save(Path::new(&a[2]),&password()?)?;},
        Some("--pack")=>{ensure!(a.len()==5,"--pack INPUT ISSUERKEY OUTPUT");packaging::pack(Path::new(&a[2]),&Issuer::load(Path::new(&a[3]),&password()?)?,Path::new(&a[4]))?;},
        Some("--enroll")=>{ensure!(a.len()==4,"--enroll PACKAGE OUTPUT.json");packaging::enroll(Path::new(&a[2]),Path::new(&a[3]))?;},
        Some("--issue")=>{ensure!(a.len()==8,"--issue ISSUERKEY PACKAGE REQUEST.json OUTPUT.lic DAYS WINDOW_SECONDS");let issuer=Issuer::load(Path::new(&a[2]),&password()?)?;let options=issuance::Options::parse(&a[6],&a[7],0)?;packaging::issue_file(&issuer,Path::new(&a[3]),Path::new(&a[4]),Path::new(&a[5]),&options)?;},
        Some("--activate")=>{ensure!(a.len()==4,"--activate PACKAGE LICENSE");let p=Package::open_exe(Path::new(&a[2]))?;let d=platform::Device::for_info(&p.info)?;let mut s=session::Session::new(p.info);s.import(Path::new(&a[3]),&d,Path::new(&a[2]))?;println!("LICENSE_ACTIVATION_PASS");},
        Some("--inspect")=>{ensure!(a.len()==3,"--inspect PACKAGE");println!("{}",serde_json::to_string_pretty(&Package::open_exe(Path::new(&a[2]))?.info)?);},
        Some("--self-check")=>verification::self_check()?,
        Some("--verify-content")=>{ensure!(a.len()>=3,"--verify-content PACKAGE");verification::content(Path::new(&a[2]),a.get(3).and_then(|s|s.parse().ok()).unwrap_or(8))?;},
        Some("--viewer-child")=>{let info=packaged.ok_or_else(||anyhow::anyhow!("没有内容包"))?;let Some(_instance)=platform::viewer_instance(&info)? else{return Ok(());};gui::run(Some((exe,info)))?;},
        Some("--help")=>println!("Encrypt your digital assets Ubuntu\nOpen without arguments for the graphical workspace.\n--create-issuer KEY | --pack INPUT KEY OUTPUT | --enroll PACKAGE REQUEST.json\n--issue KEY PACKAGE REQUEST OUTPUT.lic DAYS WINDOW_SECONDS\n--activate PACKAGE LICENSE | --inspect PACKAGE | --self-check\nPasswords are read from standard input, never command arguments.\nNo login or boot autostart is installed."),
        Some(_)=>anyhow::bail!("未知参数，请使用 --help"),
        None=>{if packaged.is_some(){let launcher=std::env::var_os("ENCRYPT_ASSETS_RUNTIME").map(|r|PathBuf::from(r).join("launch")).unwrap_or(std::env::current_exe()?);watchdog::supervise(&launcher)?;}else{gui::run(None)?;}}
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        let _ = writeln!(std::io::stderr(), "{e:#}");
        std::process::exit(1);
    }
}
