//! Encrypted application folders: stored ZIP preserves DLL/config/resource paths.
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::{Read, Write},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};
fn reparse(meta: &fs::Metadata) -> bool {
    meta.file_attributes() & 0x400 != 0
}
fn entries(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            !reparse(&meta),
            "程序目录包含链接/联接，请改用实际文件：{}",
            path.display()
        );
        if meta.is_dir() {
            out.push(path.clone());
            entries(&path, base, out)?;
        } else if meta.is_file() {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                extension.as_str(),
                "issuerkey" | "olmeta" | "josn" | "request"
            ) {
                continue;
            }
            ensure!(out.len() < 100000, "程序目录文件过多，请选择独立的程序目录");
            let relative = path.strip_prefix(base)?;
            safe_name(&relative.to_string_lossy().replace('\\', "/"))?;
            out.push(path);
        }
    }
    Ok(())
}
fn safe_name(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains(['\\', ':']) && !name.starts_with('/'),
        "资源路径无效"
    );
    for part in name.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']),
            "资源路径无效"
        );
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        ensure!(
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9')),
            "资源文件名不受支持"
        );
    }
    Ok(PathBuf::from(name))
}
pub fn pack(
    input: &Path,
    output: &Path,
    archive: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    crate::executable::validate(input)?;
    let base = fs::canonicalize(input.parent().context("程序目录无效")?)?;
    ensure!(
        base.parent().is_some(),
        "请把程序放在独立文件夹，不要直接打包盘符根目录"
    );
    let destination = fs::canonicalize(output.parent().context("输出目录无效")?)?;
    ensure!(
        !destination
            .to_string_lossy()
            .to_lowercase()
            .starts_with(&format!("{}\\", base.to_string_lossy().to_lowercase()))
            && destination.to_string_lossy().to_lowercase()
                != base.to_string_lossy().to_lowercase(),
        "请将加密 EXE 保存到原程序文件夹之外，避免把输出再次打入程序包"
    );
    ensure!(
        !fs::canonicalize(archive.parent().unwrap())?.starts_with(&base),
        "源程序目录包含本软件临时目录，请移至独立文件夹"
    );
    let mut files = vec![];
    entries(&base, &base, &mut files)?;
    files.sort();
    let total = files.iter().try_fold(0u64, |n, p| {
        Ok::<_, anyhow::Error>(
            n.checked_add(if p.is_file() {
                fs::metadata(p)?.len()
            } else {
                0
            })
            .context("程序大小溢出")?,
        )
    })?;
    let mut zip = zip::ZipWriter::new(fs::File::create(archive)?);
    let mut buffer = zeroize::Zeroizing::new(vec![0u8; crate::core::CHUNK]);
    let mut done = 0;
    for path in files {
        let size = fs::metadata(&path)?.len();
        let name = path
            .strip_prefix(&base)?
            .to_str()
            .context("程序文件名不是有效 Unicode")?
            .replace('\\', "/");
        if path.is_dir() {
            zip.add_directory(
                format!("{name}/"),
                zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )?;
            continue;
        }
        zip.start_file(
            name,
            zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .large_file(size >= u32::MAX as u64),
        )?;
        let mut input = fs::File::open(path)?;
        let mut copied = 0;
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            zip.write_all(&buffer[..n])?;
            copied += n as u64;
            done += n as u64;
            progress(done, total.max(1));
        }
        ensure!(
            copied == size,
            "程序文件在打包期间发生变化，请关闭源程序后重试"
        );
    }
    zip.finish()?.sync_all()?;
    Ok(())
}
pub fn extract(archive: &Path, dir: &Path, mut validate: impl FnMut() -> Result<()>) -> Result<()> {
    let limit = fs::metadata(archive)?.len();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    ensure!(zip.len() <= 100000, "程序包条目过多");
    let mut total = 0u64;
    let mut names = std::collections::HashSet::new();
    let mut buffer = zeroize::Zeroizing::new(vec![0u8; crate::core::CHUNK]);
    for i in 0..zip.len() {
        validate()?;
        let mut entry = zip.by_index(i)?;
        ensure!(
            (entry.is_file() || entry.is_dir())
                && entry.compression() == zip::CompressionMethod::Stored,
            "程序包条目格式无效"
        );
        let name = safe_name(entry.name().trim_end_matches('/'))?;
        ensure!(
            names.insert(entry.name().to_lowercase()),
            "程序包包含重复文件名"
        );
        ensure!(
            entry
                .unix_mode()
                .map(|m| m & 0o170000 != 0o120000)
                .unwrap_or(true),
            "程序包不能包含链接"
        );
        total = total.checked_add(entry.size()).context("程序大小溢出")?;
        ensure!(total <= limit, "程序包大小校验失败");
        let target = dir.join(name);
        if entry.is_dir() {
            fs::create_dir_all(target)?;
            continue;
        }
        fs::create_dir_all(target.parent().unwrap())?;
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        loop {
            validate()?;
            let n = entry.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            out.write_all(&buffer[..n])?;
        }
    }
    Ok(())
}
/// Delete only our fresh session subtree; never traverse reparse points.
pub fn remove_tree(dir: &Path) {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return;
    };
    if reparse(&meta) {
        return;
    }
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                remove_tree(&e.path());
            }
        }
        let _ = fs::remove_dir(dir);
    } else if meta.is_file() {
        let _ = fs::remove_file(dir);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_preserves_dependencies_and_empty_directories() -> Result<()> {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work")
            .join(format!("bundle-{}", uuid::Uuid::new_v4()));
        let source = base.join("source");
        let extracted = base.join("extracted");
        fs::create_dir_all(source.join("data"))?;
        fs::create_dir_all(source.join("empty"))?;
        fs::create_dir(&extracted)?;
        let exe = source.join("test.exe");
        fs::copy(std::env::current_exe()?, &exe)?;
        fs::write(source.join("dependency.dll"), b"dependency")?;
        fs::write(source.join("data/config.json"), b"config")?;
        fs::write(source.join("private.issuerkey"), b"not for distribution")?;
        let zip = base.join("app.zip");
        pack(&exe, &base.join("wrapped.exe"), &zip, |_, _| {})?;
        extract(&zip, &extracted, || Ok(()))?;
        assert_eq!(fs::read(extracted.join("dependency.dll"))?, b"dependency");
        assert_eq!(fs::read(extracted.join("data/config.json"))?, b"config");
        assert!(extracted.join("empty").is_dir());
        assert!(!extracted.join("private.issuerkey").exists());
        assert!(pack(&exe, &source.join("wrapped.exe"), &zip, |_, _| {}).is_err());
        remove_tree(&base);
        Ok(())
    }
    #[test]
    fn reject_escaping_and_reserved_paths() {
        for n in [
            "../a.exe",
            "/a",
            "a\\b",
            "a:stream",
            "a/CON.txt",
            "a/../b",
            "LPT1",
            "x. ",
        ] {
            assert!(safe_name(n).is_err(), "{n}");
        }
        assert!(safe_name("资源/config.json").is_ok());
    }
}
