use crate::{core::*, platform};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::Read,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

pub enum Event {
    Progress(u32, String),
    Finished(Result<Catalog>),
}
pub fn stem_name(name: &str, extension: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    format!("{stem}.{extension}")
}
pub fn metadata_path(exe: &Path, original: &str) -> PathBuf {
    exe.parent()
        .unwrap_or(Path::new("."))
        .join(stem_name(original, "olmeta"))
}
pub fn locate_metadata(exe: &Path, original: &str) -> PathBuf {
    let preferred = metadata_path(exe, original);
    for candidate in [
        preferred.clone(),
        exe.with_extension("olmeta"),
        PathBuf::from(format!("{}.olmeta", exe.display())),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    preferred
}
pub fn word(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "doc" | "docx"
    )
}
pub fn office_program() -> Option<PathBuf> {
    [
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        Some(PathBuf::from("D:/Program Files")),
        Some(PathBuf::from("D:")),
    ]
    .into_iter()
    .flatten()
    .map(|p| p.join("LibreOffice/program/soffice.exe"))
    .find(|p| p.is_file())
}
pub fn build(
    issuer: &Issuer,
    input: &Path,
    exe: &Path,
    output: &Path,
    office: Option<PathBuf>,
    sender: &Sender<Event>,
) -> Result<Catalog> {
    let dir = platform::root()
        .join("rust-admin-temp")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&dir)?;
    let report = |percent, label: &str| {
        let _ = sender.send(Event::Progress(percent, label.into()));
    };
    let result = (|| {
        report(0, "正在检查源文件…");
        let converted;
        let source = if word(input) {
            report(0, "正在转换 Word 文档，请稍候…");
            converted = convert_word(input, &dir, &office.context("转换 Word 需要 LibreOffice")?)?;
            &converted
        } else {
            input
        };
        let extension = source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind = match extension.as_str() {
            "pdf" => "pdf",
            "txt" => "text",
            "ppt" | "pptx" | "pps" | "ppsx" => "presentation",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" => "image",
            "mp4" | "mp3" | "wav" | "m4a" | "avi" | "mkv" | "mov" | "flac" | "webm" | "ogg" => {
                "media"
            }
            _ => anyhow::bail!("不支持此文件格式"),
        };
        if kind == "presentation" {
            crate::presentation::validate(source)?;
        }
        let size = fs::metadata(source)?.len();
        ensure!(size > 0, "文件为空");
        let public = issuer.spki()?;
        let info = PackageInfo {
            custom_activation_window: true,
            version: 2,
            content_id: uuid::Uuid::new_v4(),
            issuer_fingerprint: fingerprint(&public),
            issuer_public_key: b64(&public),
            display_name: input
                .file_name()
                .context("源文件名无效")?
                .to_string_lossy()
                .into(),
            kind: kind.into(),
            plain_length: size,
            chunk_size: CHUNK,
            nonce_prefix: b64(&random::<8>()),
            hardware_policy: crate::hardware::STABLE.into(),
            enrollment_public_key: issuer.enrollment_public()?,
        };
        let key = zeroize::Zeroizing::new(random::<32>());
        let package = dir.join("content.pkg");
        write_package_progress(source, &package, &info, &*key, |done, total| {
            report(5 + (done * 70 / total) as u32, "正在加密内容…")
        })?;
        report(75, "正在写入 EXE…");
        build_exe_progress(exe, &package, output, |done, total| {
            report(75 + (done * 24 / total.max(1)) as u32, "正在写入 EXE…")
        })?;
        let catalog = Catalog {
            info,
            key_base64: b64(&*key),
        };
        report(99, "正在保存管理员记录…");
        issuer.catalog_save(&metadata_path(output, &catalog.info.display_name), &catalog)?;
        report(100, "加密完成");
        Ok(catalog)
    })();
    // This directory is freshly created by this function and never contains user source files.
    let _ = fs::remove_dir_all(&dir);
    result
}
fn convert_word(input: &Path, dir: &Path, office: &Path) -> Result<PathBuf> {
    if input
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .eq_ignore_ascii_case("docx")
    {
        let mut z = zip::ZipArchive::new(fs::File::open(input)?)?;
        ensure!(z.by_name("word/document.xml").is_ok(), "DOCX 文档结构无效");
    } else {
        let mut m = [0; 8];
        fs::File::open(input)?.read_exact(&mut m)?;
        ensure!(
            m == [208, 207, 17, 224, 161, 177, 26, 225],
            "DOC 文件头无效"
        );
    }
    let profile = dir.join("profile").to_string_lossy().replace('\\', "/");
    let uri = format!(
        "file:///{}",
        profile
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"/:_-".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            })
            .collect::<String>()
    );
    let program = if office.with_extension("com").is_file() {
        office.with_extension("com")
    } else {
        office.to_owned()
    };
    let mut child = Command::new(program)
        .arg(format!("-env:UserInstallation={uri}"))
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(dir)
        .arg(input)
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Word 转换失败");
            break;
        }
        if started.elapsed() > Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Word 转换超时");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let pdf = dir.join(input.file_stem().unwrap()).with_extension("pdf");
    ensure!(pdf.is_file(), "转换没有生成 PDF");
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaging_progress_and_document_policy() -> Result<()> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("package-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let input = dir.join("逐梦.txt");
        let output = dir.join("different.exe");
        fs::write(&input, vec![42u8; CHUNK * 2 + 13])?;
        let issuer = Issuer::create()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let catalog = build(
            &issuer,
            &input,
            &std::env::current_exe()?,
            &output,
            None,
            &tx,
        )?;
        let values: Vec<_> = rx
            .try_iter()
            .filter_map(|e| {
                if let Event::Progress(n, _) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(values.last(), Some(&100));
        assert!(values.windows(2).all(|v| v[0] <= v[1]));
        assert!(dir.join("逐梦.olmeta").is_file());
        assert_eq!(
            locate_metadata(&output, "逐梦.txt"),
            dir.join("逐梦.olmeta")
        );
        let mut reader = Reader::new(Package::open_exe(&output)?, unb64(&catalog.key_base64)?)?;
        assert_eq!(&*reader.all(10_000_000)?, &fs::read(&input)?);
        for kind in ["presentation", "pdf", "text"] {
            let mut info = catalog.info.clone();
            info.kind = kind.into();
            assert!(!crate::watchdog::enabled_for(&info));
            assert!(crate::watchdog::enable_startup(Path::new("nonexistent.exe"), &info).is_err());
            assert!(crate::watchdog::Client::start(Path::new("nonexistent.exe"), &info).is_err());
        }
        for kind in ["media", "image"] {
            let mut info = catalog.info.clone();
            info.kind = kind.into();
            assert!(crate::watchdog::enabled_for(&info));
        }
        fs::rename(dir.join("逐梦.olmeta"), dir.join("different.exe.olmeta"))?;
        assert_eq!(
            locate_metadata(&output, "逐梦.txt"),
            dir.join("different.exe.olmeta")
        );
        // An invalid input must report failure before replacing an existing output.
        let invalid = dir.join("invalid.pptx");
        fs::write(&invalid, b"not a ZIP")?;
        assert!(build(
            &issuer,
            &invalid,
            &std::env::current_exe()?,
            &output,
            None,
            &tx
        )
        .is_err());
        assert_eq!(Package::open_exe(&output)?.info, catalog.info);
        fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn original_names() {
        assert_eq!(stem_name("逐梦.MP4", "olmeta"), "逐梦.olmeta");
        assert_eq!(stem_name("逐梦.pptx", "josn"), "逐梦.josn");
        assert_eq!(stem_name("逐梦.pptx", "lic"), "逐梦.lic");
        assert_eq!(stem_name("会议.最终.docx", "josn"), "会议.最终.josn");
    }
}
