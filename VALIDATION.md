# Windows 2.5.0 · Validation / 验证范围

Updated: 2026-10-20 (Asia/Shanghai).

- Default Rust suite: 23 passed, 1 native desktop fixture test ignored.
- Actual Windows UI: header language button, attached window icon, 12 controls in minimum/normal/large windows with no clipping or overlaps.
- Nested modal painting, minimize/restore and 60 resize/language cycles passed; GDI count remained stable.
- Chinese/English input preservation, synthetic encrypted text/video preview, device/setup/recipient entry points passed.
- Custom import window: GUI 5-minute/30-day LIC issuance/import, cancellation without output, remembered settings and CLI 2-hour window passed.
- Actual synthetic packaged MP4: three loops in one PID, hidden full-screen controls, pause/resume, seek and fresh-process startup authorization recovery passed. No actual reboot or logoff.
- Parsed the production Packager/DeviceTool/Setup PE resources: all seven ICO payload sizes match the source icon, and the common-controls manifest is present.
- Gemini independently executed the Windows unit, identity/layout, repaint, language/preview and PE-resource commands; all returned code 0. Codex inspected the screenshots and ran the custom-window/media checks.

Screenshots in this revision show a test build with a synthetic unlocked issuer/package; they contain no real hardware data or issuer material. Production binaries are separately built without watchdog-test-mode. Windows artwork remains a full-window library background plus the new lock-holding mascot, with provenance in docs/.

Not revalidated: mixed-monitor DPI, all Windows themes/versions, all codecs, full PowerPoint animation fidelity, real login/reboot and long-running kiosk behavior. The existing Windows startup policy is retained; Ubuntu has no startup registration.

## Reproduce

Build `cargo build --locked --features watchdog-test-mode`, then run `build/identity-ui-test.ps1`, `build/paint-ui-test.ps1 -Version 2.5.0`, `build/ui-test.ps1 -SkipBuild`, `build/window-ui-test.ps1`, and `python build/check-icons.py`. GUI scripts must run sequentially on an interactive desktop. Use `build/video-e2e.ps1 -Formats mp4` with a short synthetic `work/sample.mp4`; do not automate real reboot/logoff.

Historical 2.3–2.4 tests remain available in their release tags. Do not treat those full container/Office checks as rerun by this UI-only update.
