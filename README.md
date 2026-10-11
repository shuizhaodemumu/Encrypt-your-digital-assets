# Offline Media License · Rust

[中文](README.zh-CN.md) | English

Windows desktop software for packaging audio, video and documents into encrypted standalone EXEs with offline, hardware-bound licensing. Version **2.2.0** adds Chinese / English menus, dialogs, status messages and validation errors to the Rust 2.1.1 Office edition.

## Features

- **Video and audio encryption:** chunked AES-256-GCM encryption; licensed media is read through the decrypted stream by LibVLC. Videos play full-screen without controls and loop one file until the license expires. Native input repetition retains the video output across loop boundaries; the video canvas and fullscreen background are black.
- **Offline licensing:** encrypted device reports, issuer signatures, hardware binding, a 60-second first-activation window, encrypted issuer-key backups and Windows DPAPI-protected local state.
- **Documents:** built-in PDF, text and image viewing. Word DOC/DOCX conversion uses separately installed LibreOffice on the administrator PC.
- **Native PowerPoint playback:** original PPT/PPTX/PPS/PPSX files are encrypted, then shown through desktop Microsoft PowerPoint on the recipient PC. PowerPoint must be installed and activated. This route uses a temporary decrypted file; read-only playback does not provide absolute protection from extraction.
- **Chinese / English interface:** use the 中文 or English menu on any application window. The selection is remembered for this Windows user. Switching labels does not restart playback or change content, issuer keys or licensing data. System file-picker buttons follow the Windows display language.

## Applications

| Executable | Purpose |
| --- | --- |
| `Packager.exe` | Create/unlock issuer keys, preview files, build content EXEs and issue licenses |
| `DeviceTool.exe` | Select a content EXE and export an encrypted device report |
| `Packager-Setup.exe` | Install the administrator application |
| Generated content EXE | Export a device report, import a LIC and view/play the licensed content |

The bilingual edition uses `%LOCALAPPDATA%/OfflineLicenseBilingualRust` for its own installation and state. Existing Office-edition installations and device keys are preserved. Use a device report produced by this edition when issuing its licenses.

## Build on Windows x64

Install stable Rust using [rustup](https://rustup.rs/) with either the MSVC build tools or a suitable GNU toolchain. From the repository root:

```powershell
cargo test --locked
cargo build --locked --release
powershell -NoProfile -ExecutionPolicy Bypass -File build/build.ps1 -Release
```

The helper copies the same application binary to the three executable names in `程序/`. Runtime components in `assets/runtime.zip` are embedded in the binary; it is required for building. See [third-party notices](第三方说明.md) and `third-party-licenses/` for component notices and source locations.

For UI regression, build with `--features watchdog-test-mode` before running `build/ui-language.ps1`. That feature disables actual system restart behavior; test binaries must not be distributed as the production release. Automated checks do not reboot the computer.

## Upgrade existing Rust Office content

Existing encrypted EXEs contain their own player, so replacing only the packager cannot fix them. Build the Office-compatible updater:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build/build.ps1 -Release -OfficeCompat
```

Place `build/Upgrade-Content.ps1` and `build/升级已有内容.cmd` beside `程序/ViewerUpdater-OfficeCompat.exe`, then run the CMD helper and select the original EXE and a new output filename. The updater verifies that encrypted payload bytes remain identical and copies the administrator record. It uses the original `OfflineLicenseOfficeRust` state namespace for Rust Office 2.1.1 compatibility; it does not extend or replace licenses. Close the old content application before opening the upgraded one. Keep the original file. Older C# and version 1 packages are not upgraded by this route.

The native-loop integration test is deliberately excluded from ordinary `cargo test` because it needs a short MP4 and a Windows desktop. Run `build/loop-test.ps1 -Fixture path/to/short.mp4` to test four repeats, output-window continuity, edge-color sampling and authorization revocation. It uses a temporary encrypted package and an in-memory authorization fixture; it does not collect hardware or install/startup-register a viewer.

## Basic workflow

1. Create an issuer key and an encrypted backup; keep both on the administrator PC.
2. Import a file, preview it, and build its encrypted content EXE. Retain the `.olmeta` administrator record.
3. Send the content EXE. The recipient exports its encrypted device report.
4. Import that report, choose the number of days, and issue a `.lic` file.
5. The recipient imports the license within 60 seconds. The licensed term begins upon activation.

Keys, real device reports, licenses, content records, user documents and local test artifacts are excluded from the source repository. Runtime monitoring/login startup behavior remains part of the original application; install and playback actions are explicit user actions.

## Verification scope

See [VALIDATION.md](VALIDATION.md) for results and limitations. Original PowerPoint 16.0 verification belongs to the 2.1.1 baseline; language changes do not constitute testing every Office version or Word conversion.
