# Validation / 验证范围

Updated: 2026-10-16 (Asia/Shanghai). Source version: 2.4.3.

| Scope | Actual result | Limit |
| --- | --- | --- |
| Default Rust suite | 23 passed, 0 failed, 1 ignored | The ignored native video test needs a fixture and Windows desktop. |
| 2.4.0 licensing | GUI 5-minute window / 30-day term, confirmation cancel, settings memory, CLI 2-hour window and recipient import passed | Synthetic issuers; no production credentials published. |
| 2.4.0 video regression | Real packaged MP4: same PID for 3 loops, pause/resume, seek, fullscreen/dock and registered-copy auth recovery passed | No actual reboot or logoff; one fixture, not all codecs. |
| 2.3.0 container matrix | MP4/MKV/AVI/MOV/WEBM native and GUI checks recorded | Earlier revision scope; not rerun as a full matrix after every UI-only change. |
| 2.4.1 repaint | Modal nested owner-draw, minimize/restore and 60 resize/language cycles passed | Mixed-monitor DPI and GDI exhaustion not covered. |
| 2.4.2 artwork | Actual UI inspected by Codex; source-library image and visible illustration checks passed | H3 replacement generation failed its memory guard; not counted as generated art. |
| 2.4.3 feathering | Gemini reran paint-ui successfully after a PowerShell test expression fix; language-ui passed separately | Codex then added explicit border/contrast PASS logs and reran locally; this last logging-only run was not a new Gemini run. |

The automated GUI/media checks use watchdog-test-mode to avoid a real Windows reboot. The production build does not use that feature. Fresh OS installation, actual reboot/login, every Office version, every codec and long-running kiosk deployment are not fully revalidated by these results. Native VLC diagnostic warnings are not claimed eliminated.

## Reproducing desktop checks

Use an interactive Windows desktop. Build with cargo build --locked --features watchdog-test-mode, then run build/ui-test.ps1 -SkipBuild and build/paint-ui-test.ps1 -Version 2.4.3 -CheckSideArtwork -FrameFree. UI scripts preserve language/issuance preferences and create synthetic data under work/. Test prerequisites are not distributed as personal device reports or keys.

For build/video-e2e.ps1 -Formats mp4 provide a short work/sample.mp4. For build/native-matrix.ps1 provide equivalent sample.mp4/mkv/avi/mov/webm files in work/. Do not run GUI scripts concurrently. Do not automate real reboot/logoff. PowerPoint must be installed and activated for real slideshow acceptance; LibreOffice is needed only for Word conversion.
