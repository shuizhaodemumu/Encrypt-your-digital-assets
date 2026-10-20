use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-env-changed=WINDRES");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let gnu = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let artifact = out.join(if gnu { "app-icon.o" } else { "app-icon.res" });
    fs::copy("assets/app-icon.ico", out.join("app-icon.ico")).unwrap();
    fs::copy("assets/app.rc", out.join("app.rc")).unwrap();
    fs::copy("assets/app.manifest", out.join("app.manifest")).unwrap();
    let mut command = if gnu {
        let mut c = Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into()));
        c.args([
            "--input-format=rc",
            "--output-format=coff",
            "-i",
            "app.rc",
            "-o",
        ]);
        // GNU windres uses the active ANSI codepage for absolute arguments.
        // Keep its output path relative so a Chinese workspace name is safe.
        c.arg("app-icon.o");
        c
    } else {
        let rc = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
            let kits = PathBuf::from(
                env::var_os("ProgramFiles(x86)")
                    .unwrap_or_else(|| r"C:\Program Files (x86)".into()),
            )
            .join("Windows Kits/10/bin");
            let mut versions: Vec<_> = fs::read_dir(kits)
                .into_iter()
                .flatten()
                .flatten()
                .map(|d| d.path().join("x64/rc.exe"))
                .filter(|p| p.is_file())
                .collect();
            versions.sort();
            versions.pop().unwrap_or_else(|| "rc.exe".into())
        });
        let mut c = Command::new(rc);
        c.arg("/nologo").arg("/fo").arg(&artifact).arg("app.rc");
        c
    };
    let status = command
        .current_dir(&out)
        .status()
        .expect("Windows resource compiler is required (windres or Windows SDK rc.exe)");
    assert!(
        status.success(),
        "Application icon resource compilation failed"
    );
    println!("cargo:rustc-link-arg={}", artifact.display());
}
