use crate::{core::*, hardware};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
pub fn kind(path: &Path) -> Result<&'static str> {
    Ok(
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "mp3" | "wav" | "m4a" | "flac" | "ogg" => {
                "media"
            }
            "pdf" | "doc" | "docx" | "odt" => "pdf",
            "txt" => "text",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" => "image",
            "ppt" | "pptx" | "pps" | "ppsx" | "odp" => "presentation",
            _ => anyhow::bail!("不支持的文件类型"),
        },
    )
}
pub fn is_video(name: &str) -> bool {
    matches!(
        Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "mkv" | "avi" | "mov" | "webm"
    )
}
pub fn catalog_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".olmeta");
    s.into()
}
pub fn pack(source: &Path, issuer: &Issuer, output: &Path) -> Result<()> {
    let source = source.canonicalize()?;
    ensure!(source.is_file(), "请选择普通文件");
    ensure!(!output.exists(), "输出文件已存在，请选择新名称");
    let sidecar = catalog_path(output);
    ensure!(!sidecar.exists(), "管理员记录已存在，请选择新名称");
    let kind = kind(&source)?;
    let temp = tempfile::Builder::new()
        .prefix("encrypt-assets-pack-")
        .tempdir()?;
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let input = if matches!(extension.as_str(), "doc" | "docx" | "odt") {
        convert_document(&source, temp.path())?
    } else {
        source.clone()
    };
    let mut name = source
        .file_name()
        .context("文件名无效")?
        .to_string_lossy()
        .into_owned();
    if input != source {
        name = Path::new(&name)
            .with_extension("pdf")
            .to_string_lossy()
            .into_owned();
    }
    let spki = issuer.spki()?;
    let info = PackageInfo {
        version: 2,
        custom_activation_window: true,
        content_id: uuid::Uuid::new_v4(),
        issuer_fingerprint: fingerprint(&spki),
        issuer_public_key: b64(&spki),
        display_name: name,
        kind: kind.into(),
        plain_length: fs::metadata(&input)?.len(),
        chunk_size: CHUNK,
        nonce_prefix: b64(&random::<8>()),
        hardware_policy: hardware::STABLE.into(),
        enrollment_public_key: issuer.enrollment_public()?,
    };
    ensure!(info.plain_length > 0, "输入文件为空");
    let key = Zeroizing::new(random::<32>());
    let encrypted = temp.path().join("content.pkg");
    write_package(&input, &encrypted, &info, &*key)?;
    let template = crate::template_path()?;
    ensure!(
        Package::open_exe(&template).is_err(),
        "请使用管理员程序打包"
    );
    build_exe(&template, &encrypted, output)?;
    if let Err(e) = issuer.catalog_save(
        &sidecar,
        &Catalog {
            info,
            key_base64: b64(&*key),
        },
    ) {
        let _ = fs::remove_file(output);
        return Err(e);
    }
    Ok(())
}
fn convert_document(source: &Path, dir: &Path) -> Result<PathBuf> {
    let profile = format!(
        "-env:UserInstallation=file://{}",
        dir.join("profile").display()
    );
    let mut child = Command::new("libreoffice")
        .args([
            "--headless",
            "--nologo",
            "--norestore",
            &profile,
            "--convert-to",
            "pdf",
            "--outdir",
        ])
        .arg(dir)
        .arg(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("转换 Word/ODT 需要安装 LibreOffice Writer")?;
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait()? {
            ensure!(s.success(), "LibreOffice 文档转换失败");
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("文档转换超时");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = dir
        .join(source.file_stem().context("文件名无效")?)
        .with_extension("pdf");
    ensure!(out.is_file(), "未生成 PDF 文档");
    Ok(out)
}
pub fn enroll(package: &Path, output: &Path) -> Result<()> {
    let p = Package::open_exe(package)?;
    let device = crate::platform::Device::for_info(&p.info)?;
    atomic(output, &encode(&hardware::export(&p.info, &device)?)?)
}
pub fn issue_file(
    issuer: &Issuer,
    package: &Path,
    request: &Path,
    output: &Path,
    options: &crate::issuance::Options,
) -> Result<()> {
    let info = Package::open_exe(package)?.info;
    let catalog = issuer.catalog_load(&catalog_path(package))?;
    ensure!(info == catalog.info, "内容文件和管理员记录不一致");
    let req = hardware::request_for(issuer, &info, request)?;
    let lic = issue_with_window(
        issuer,
        &catalog,
        &req,
        options.days,
        options.validate(now())?,
        now(),
    )?;
    ensure!(!output.exists(), "授权文件已存在，请使用新名称");
    options.save()?;
    atomic(output, &encode(&lic)?)?;
    Ok(())
}
