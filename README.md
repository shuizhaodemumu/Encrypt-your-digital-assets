# Encrypt your digital assets

[中文说明](README.zh-CN.md) · **Version 2.4.3** · Windows x64 · Rust / Win32

Package a video, presentation, image or document as an encrypted Windows EXE, then issue an offline license for the recipient's device. The recipient exports an encrypted device request; the administrator returns a signed `.lic` file. No online activation service is required.

![The 2.4.3 licensing workspace](docs/screenshots/workspace-zh.png)

## What it does

| Content / capability | Current behavior |
| --- | --- |
| Video | MP4, MKV, AVI, MOV and WEBM use embedded LibVLC. Licensed videos open full-screen, hide controls and repeat the same file without restarting the EXE at the end. MOV uses the bundled avformat demuxer. |
| Audio | MP3, WAV, M4A, FLAC and OGG use LibVLC with playback controls. Video looping policy is not automatically applied to all audio. |
| PowerPoint | Encrypts original PPT/PPTX/PPS/PPSX files and uses desktop PowerPoint COM automation for slide-show-only playback. It does **not** flatten slides into PDF; animations and transitions remain subject to PowerPoint compatibility. |
| PDF / text / images | PDFium renders PDF through memory callbacks; text and supported images are displayed in the built-in viewer. Image inputs include PNG/JPEG/GIF/BMP/TIFF; full animated-image playback is not claimed. |
| Word | The administrator uses a separately installed LibreOffice to convert DOC/DOCX to PDF before packaging. Recipients view the converted pages; this is not an editable Word document. |
| Licensing | Device-bound signed LIC, configurable first-import window, activation-based usage duration, encrypted issuer-key backups and DPAPI-protected local state. |
| Login startup | After activation, a verified installed copy is registered under the current user's Windows `Run` key. This runs **after login**, not as a pre-login service. A repair action and CLI removal command are available. |
| Fault recovery | A heartbeat supervisor restarts faulty/unresponsive playback. The production policy can restart Windows after the third fault restart within ten minutes. Normal pause, close, license expiry and successful video loops are not faults. |
| Interface | Chinese/English menus and dialogs, live input validation, action prerequisites, confirmation before issuance and saved valid settings. 2.4.3 removes the illustration card frame and blends its edges into the pale full-window background. |

Container support does not guarantee every codec, resolution or GPU driver. PowerPoint and LibreOffice are external prerequisites; neither is bundled.

## Use the application

Download the current portable package from [Releases](https://github.com/shuizhaodemumu/Encrypt-your-digital-assets/releases). Extract it to a writable directory and run `Packager.exe`. Building from source produces `Packager.exe`, `DeviceTool.exe` and `Packager-Setup.exe` in `程序/`. Binary names and the internal Cargo package remain stable for compatibility; the project display name is **Encrypt your digital assets**.

1. Create or unlock the issuer key. Keep the `.issuerkey`, password and administrator-only `.olmeta` content catalog private.
2. Import and preview the original content, then build its encrypted EXE. Send that EXE to the recipient.
3. On first launch, the recipient selects **Get this device's identification code** and returns the encrypted JSON request (`.json`, with legacy `.josn` naming supported by the workflow).
4. Select the matching content package in the administrator UI. Set **license days** and the **first-import window**, then import the request.
5. Review the confirmation and issue the LIC. Send it to the recipient, who imports it before its first-import deadline.
6. The content opens after verification. The license term starts at the recipient's successful first activation. Startup registration and the supervisor are enabled for activated content.

### Two independent time settings

- **First-import window:** a positive whole number in seconds, minutes, hours or days; initially **60 seconds**. The countdown starts when issuance is confirmed. A 2-hour setting allows two hours to deliver and import the LIC.
- **Usage duration:** a positive number of days starting at first successful activation, not at issuance.

Empty, zero, negative, fractional and overflowing values are rejected. The last valid configuration is remembered. Canceling the confirmation produces no LIC. A new license replaces the previous term rather than adding days. The signed deadline cannot be extended by editing the LIC.

Older content EXEs embed their old validator. Repackage the original content with this version to use a custom window; the administrator rejects a non-60-second window for old packages. New readers still accept historical 60-second licenses.

### Video controls

Move the pointer to the bottom edge to reveal the controls; they hide again after a short delay. Space toggles playback, Left/Right seek ten seconds, F11 toggles full-screen, and Esc leaves full-screen (or exits from windowed mode). End-of-video repetition stays in the same process.

Remove a content startup registration with:

```powershell
.\程序\Packager.exe --disable-startup "C:\path\to\content.exe"
```

Do not delete the entire shared user-data directory to disable one content item: device keys can be shared by multiple packages.

## How it works

```text
Original file -> chunk encryption -> player EXE + encrypted payload
                                      |
Recipient hardware + device public key -> encrypted request
                                      |
Administrator issuer key + content catalog -> signed, device-wrapped LIC
                                      |
Recipient verifies signature / package / device / time -> opens content
```

- **Content:** random content key; 1 MiB AES-256-GCM chunks with authenticated metadata. The payload is appended to the player executable. Random-access readers decrypt chunks as required by PDF/media callbacks.
- **LIC:** ECDSA P-256 signs the license body; RSA-OAEP/SHA-256 wraps the content key for the recipient device. Content identity, issuer, hardware policy and time constraints are checked before use.
- **Device request:** ephemeral P-256 ECDH, HKDF-SHA-256 and AES-256-GCM protect the hardware report for the issuer.
- **Issuer backup:** PKCS#8 password encryption using PBKDF2-HMAC-SHA-256 (600,000 iterations) and AES-256-CBC. Local device keys and activation state use Windows DPAPI.
- **Hardware:** Windows PowerShell/CIM collects computer, board, product UUID, disk, CPU, BIOS, memory and physical-network-adapter fields. Stable binding excludes USB removable storage and changing connection status; the full collected hardware report remains encrypted. IP addresses, DNS and public-IP geolocation are excluded. Changes to bound hardware can require reissuance; stability is not tolerance of every hardware replacement.
- **Rendering:** native Win32 controls with offscreen painting and cached frames during nested modal repaint. The right-hand illustration uses premultiplied alpha and `AlphaBlend`; it is hidden when a content preview occupies the workspace.
- **Runtime:** LibVLC DLLs/plugins and PDFium are embedded in `assets/runtime.zip` and extracted locally. PowerPoint uses a separate host/COM route; Word conversion calls LibreOffice only on the administrator side.

### Protection limits

This is offline licensing and encrypted distribution, not a guarantee that content can never be copied. Authorized viewing exposes pixels/audio, and a recipient controlling the OS can record the screen, inspect memory or modify binaries. Native PowerPoint needs a temporary decrypted presentation file; read-only slide-show restrictions do not eliminate extraction. A fully offline client cannot obtain trusted server time or remote revocation. Keep issuer keys and content catalogs private; encrypted device reports are still sensitive administrator data.

## Build from source

Use Windows x64, a recent stable Rust toolchain (edition 2021) and a matching Windows linker. A standard Rust MSVC setup needs Visual Studio C++ Build Tools / Windows SDK; GNU builds need an appropriate MinGW/LLVM linker. `Cargo.lock` pins Rust dependencies.

```powershell
git clone https://github.com/shuizhaodemumu/Encrypt-your-digital-assets.git
cd Encrypt-your-digital-assets
cargo fmt --check
cargo test --locked
cargo build --locked --release
# Or build and copy all three application entry points:
.\build\build.ps1 -Release
```

The PowerShell helper supports the existing local GNU SDK layout through `OFFLINE_RUST_SDK`, and falls back to `cargo.exe` on PATH. Ordinary builds do not require the author's D-drive directory layout. Direct Cargo output is `target/release/offline-license-rust.exe`; the packaging script creates the three role-specific filenames.

### Testing

Desktop scripts need an interactive Windows desktop and a **test build**, plus the synthetic fixtures described in [VALIDATION.md](VALIDATION.md):

```powershell
cargo build --locked --features watchdog-test-mode
.\build\ui-test.ps1 -SkipBuild
.\build\paint-ui-test.ps1 -Version 2.4.3 -CheckSideArtwork -FrameFree
# Optional media end-to-end regression with a short work/sample.mp4 fixture:
.\build\video-e2e.ps1 -Formats mp4
```

`watchdog-test-mode` disables real machine reboot for tests. Do not enable it in production releases. Do not use real reboot/logoff as an automated acceptance check. Hardware-report/LIC tests create local synthetic issuer data; do not publish their work directories.

23 normal tests passed and one desktop-dependent native test remains ignored in the default suite. Actual GUI checks cover bilingual dialogs, content preview, custom-window issuance/import, modal repaint, borderless illustration pixels and 60 resize/language cycles. MP4 looping/startup recovery and earlier five-container checks have narrower recorded scopes; see [validation history](VALIDATION.md). There is no blanket claim of reboot/login, all codecs, mixed-DPI monitors or every Office version being tested.

## Source map

| File | Responsibility |
| --- | --- |
| `src/core.rs` | Package format, chunk crypto, key storage and signed licenses |
| `src/hardware.rs`, `assets/hardware.ps1` | Hardware policy and encrypted enrollment |
| `src/issuance.rs`, `src/session.rs` | Editable import window, activation and runtime authorization |
| `src/native.rs` | LibVLC/PDFium FFI and encrypted read/seek callbacks |
| `src/presentation.rs`, `src/automation.rs` | PowerPoint host, COM automation and presentation lifecycle |
| `src/platform.rs`, `src/install.rs`, `src/watchdog.rs` | DPAPI, installation/startup and heartbeat recovery |
| `src/gui.rs`, `src/i18n.rs` | Win32 UI, bilingual state and layered painting |
| `build/` | Build and Windows GUI/media regression scripts |

## Third-party components and artwork

See [third-party notices](第三方说明.md) and `third-party-licenses/` for LibVLC, PDFium and Rust dependency notices. The executable uses dynamic native libraries; there is no .NET/LibVLCSharp dependency in the Rust application. Third-party licenses are not a blanket license for all project artwork.

The right-hand illustration is from the user-provided reference library; its [source index](docs/artwork-provenance.json) records the original attribution. H3 generation attempts for that replacement hit a memory guard, so **the shipped illustration is a library image, not a newly generated H3 result**. Runtime feathering is implemented in Rust.

Private keys, LIC files, device reports, local SDKs, test workspaces, old backups and compiled application bundles are excluded from source commits. Prebuilt downloads belong in Releases rather than source history.
