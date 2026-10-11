use crate::{core::*, platform::wide};
use anyhow::{ensure, Result};
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use windows_sys::Win32::{Foundation::*, System::Registry::*};
fn root() -> PathBuf {
    crate::platform::root().join("apps")
}
pub fn install(source: &Path, info: Option<&PackageInfo>) -> Result<PathBuf> {
    let id = info
        .map(|i| format!("content-{}", i.content_id.simple()))
        .unwrap_or("administrator".into());
    ensure!(
        id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        crate::i18n::text("安装编号无效")
    );
    let dir = root().join(&id);
    ensure!(
        id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        crate::i18n::text("安装编号无效")
    );
    fs::create_dir_all(&dir)?;
    let target = dir.join(if info.is_some() {
        "Content.exe"
    } else {
        "Packager.exe"
    });
    if fs::canonicalize(source).ok() != fs::canonicalize(&target).ok() {
        let temp = target.with_extension("new");
        fs::copy(source, &temp)?;
        crate::platform::replace_file(&temp, &target)?;
    }
    if info.is_none() {
        fs::copy(source, dir.join("DeviceTool.exe"))?;
        fs::copy(source, dir.join("Packager-Setup.exe"))?;
    }
    let name = info
        .map(|i| format!("{} · Rust双语版", i.display_name))
        .unwrap_or("离线文件授权管理员 · Rust双语版".into());
    let safe: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .take(70)
        .collect();
    let ps=format!("$s=New-Object -ComObject WScript.Shell; foreach($folder in @([Environment]::GetFolderPath('DesktopDirectory'),[Environment]::GetFolderPath('Programs'))){{$path=Join-Path $folder '{}-{}.lnk';$l=$s.CreateShortcut($path);$l.TargetPath='{}';$l.WorkingDirectory='{}';$l.Save()}}",safe.replace('\'',"''"),id,target.display().to_string().replace('\'',"''"),dir.display().to_string().replace('\'',"''"));
    let encoded = b64(&ps
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>());
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    ensure!(
        child.wait()?.success(),
        crate::i18n::text("创建快捷方式失败")
    );
    unsafe {
        let mut key = 0;
        let namekey = wide(&format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}-{id}",
            crate::platform::STATE_NAMESPACE
        ));
        ensure!(
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                namekey.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut()
            ) == ERROR_SUCCESS,
            crate::i18n::text("安装记录写入失败")
        );
        let uninstall = if info.is_some() {
            format!("\"{}\" --uninstall", target.display())
        } else {
            format!(
                "\"{}\" --uninstall",
                dir.join("Packager-Setup.exe").display()
            )
        };
        for (n, v) in [
            ("DisplayName", name),
            ("InstallLocation", dir.display().to_string()),
            ("UninstallString", uninstall),
        ] {
            let data = wide(&v);
            RegSetValueExW(
                key,
                wide(n).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            );
        }
        RegCloseKey(key);
    }
    Ok(target)
}
pub fn uninstall(info: Option<&PackageInfo>, remove_state: bool) -> Result<()> {
    let namespace = crate::platform::STATE_NAMESPACE;
    if let Some(info) = info {
        crate::watchdog::disable_startup(info)?;
    }
    let id = info
        .map(|i| format!("content-{}", i.content_id.simple()))
        .unwrap_or("administrator".into());
    let dir = root().join(&id);
    if remove_state {
        if let Some(i) = info {
            let p = crate::platform::state_path(i.content_id);
            if p.exists() {
                fs::remove_file(p)?;
            }
        }
    }
    let escaped = dir.display().to_string().replace('\'', "''");
    let script=format!("Start-Sleep -Seconds 2;$target='{escaped}';$resolved=[IO.Path]::GetFullPath($target);if(!$resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::Combine([Environment]::GetFolderPath('LocalApplicationData'),'{namespace}','apps'))+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){{throw 'Invalid uninstall target'}};for($i=0;$i -lt 20;$i++){{try{{if(Test-Path -LiteralPath $target){{Remove-Item -LiteralPath $target -Recurse -Force}};break}}catch{{Start-Sleep -Seconds 1}}}};foreach($folder in @([Environment]::GetFolderPath('DesktopDirectory'),[Environment]::GetFolderPath('Programs'))){{Get-ChildItem -LiteralPath $folder -Filter '* · Rust双语版-{}.lnk' | Remove-Item -Force}}",id);
    let encoded = b64(&script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>());
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .creation_flags(0x08000000)
        .spawn()?;
    unsafe {
        RegDeleteTreeW(
            HKEY_CURRENT_USER,
            wide(&format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}-{id}",
                crate::platform::STATE_NAMESPACE
            ))
            .as_ptr(),
        );
    }
    Ok(())
}
