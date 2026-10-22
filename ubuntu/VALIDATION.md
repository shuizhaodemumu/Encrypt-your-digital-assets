# Ubuntu 0.1.3 · Video control auto-hide regression
- User reported that controls remained visible in real playback even though the 0.1.2 automated test passed. Review found the test recorded the requested reveal state before GTK completed the animation; it did not prove the widget was actually hidden.
- The motion handler now resets the idle clock only when pointer coordinates change. The controls slide down after 1 second idle; real mouse movement slides them out.
- Regression instrumentation now records `GtkRevealer::is_child_revealed()` after the transition, and the test asserts the 1-second hide completes within 2 seconds.
- Validation: Rust format check and optimized watchdog test build passed. Xvfb + D-Bus GTK regression passed: `CONTROL_BAR_IDLE_1S_SLIDES_TO_BOTTOM_PASS (1.04s)` and `CONTROL_BAR_MOUSE_MOTION_SLIDES_OUT_PASS`.
- Production build passed. Thirteen Rust tests passed; the `.deb` metadata reports `encrypt-assets-ubuntu 0.1.3 amd64`, the bundled RUN reports `Encrypt your digital assets Ubuntu 0.1.3`, and the DEB, RUN, and source archive have SHA-256 entries in `outputs/SHA256SUMS.txt`.
- The 0.1.3 DEB was staged in the Ubuntu VM Downloads folder and its hash matched the build output. Follow-up acceptance on 2026-10-22 purged 0.1.2, installed 0.1.3, confirmed the installed package version with `dpkg-query`, and opened the application from its desktop entry; the user accepted the release. No actual system restart or autostart behavior is involved in this UI change.


Recorded on 2026-10-21, Ubuntu 26.04.1 x86_64 in VirtualBox.

- `cargo fmt --check`, locked Rust tests (13 passed), and optimized release build passed.
- The original 0.1.2 GTK regression checked only the requested state and was not sufficient to confirm the completed hide animation; see the 0.1.3 correction above.
- Production DEB/RUN were rebuilt. Their embedded application binaries match; desktop entries and all eight installed icon sizes passed, and the production RUN opened its administrator window without creating startup entries.
- The isolated test RUN disabled networking and hid host GTK/VLC/Office/Python components. MP4, M4A, PDF, DOCX and PPTX completed pack → encrypted device request → LIC issuance → activation → playback/viewing. MP4 playback repeated three times in the same player. Normal close, cleanup of application/Office child processes, and unchanged autostart state passed.
- The full functional matrix uses the `watchdog-test-mode` package so fault checks cannot reboot the VM. The DEB was inspected and extracted, not installed system-wide. A human review of the released interface on a physical Ubuntu desktop was not performed in this release check.

---

# Ubuntu 0.1.1 · Icon update validation

Recorded on 2026-10-20, Ubuntu 26.04.1 x86_64.

- Rust formatting, test/production builds and all 13 Rust tests passed.
- GTK interaction regression passed: prerequisites, invalid/recovered import windows, preserved language/input state and native chooser cancellation.
- DEB and RUN contain identical production launcher bytes; both desktop entries validate and eight installed icon sizes match their source PNGs.
- The production RUN launched with isolated state. The visible X11 main window provides its icon; per-user menu entries and the embedded fallback icon match the shipped asset. No boot/login autostart directory was created.
- The GTK application ID and desktop filename now match for Wayland shell association; Codex also inspected the production app and its new Dock icon on the actual GNOME desktop. GTK's hidden application-leader window is intentionally excluded from the window-icon assertion.
- Gemini's first round passed six commands and failed the icon assertion because the test selected that hidden helper window. The corrected test checks only the visible window; the second round independently reran GTK and icon checks with exit code 0. Its menu-category hint was addressed by selecting one main category and requiring a clean desktop-file-validate output.

The 0.1.1 update changes application identity/icons, not encryption, licensing, media or Office logic. The full format/offline media matrix below belongs to 0.1.0 and was not repeated for every icon-only edit. Installation checks in this update cover the DEB payload and actual RUN execution, not a new privileged system-wide DEB installation. Build-time installer documentation can precede the final release validation record.

---

# Ubuntu 0.1.0 · Validation

Recorded on 2026-10-19. This summary publishes results without raw device reports, user paths, issuer keys or private test state. It is separate from the root Windows [validation history](../VALIDATION.md).

## Tested environment

Ubuntu 26.04.1 x86_64 in VirtualBox; Rust 1.93.1, GTK 4.22.4, LibVLC 3.0.23, Poppler 26.01.0 and LibreOffice 26.2. Automated media/fault checks used `watchdog-test-mode`; production binaries were separately built, installed and launched on the GNOME desktop.

| Check | Observed result |
| --- | --- |
| Rust formatting, compile, production and test builds | Passed; 13 Rust tests passed, including encryption/signatures, replay handling, device policy, enrollment, import windows, saved language and rolling fault counts. |
| Pack → request → issue → activate → native view | Passed with 17 synthetic formats: MP4/MKV/AVI/MOV/WEBM; WAV/MP3/M4A/FLAC/OGG; TXT/PNG/PDF/DOCX/PPTX/PPT/ODP. |
| Video/audio | Each of five video formats looped at least three times in the same player. Audio checks observed decoded blocks. Forged signatures were rejected; normal close did not cause a fault restart. |
| Documents | PDF memory rendering, Word conversion/encrypted viewing, and original PPTX/PPT/ODP read-only UNO slideshows with page changes passed. |
| GTK UI | Actual Xvfb interaction covered prerequisites, invalid/recovered import windows, language switches retaining drafts and file-dialog cancellation; screenshots were inspected and non-black assertions added. |
| Offline bundle | bwrap disabled networking and hid host GTK/VLC, Office, Python and fonts. MP4/M4A/PDF/DOCX/PPTX completed the workflow from Chinese/space-containing paths. Bundled Noto CJK was selected. |
| Install/cleanup | Concurrent first launches installed the shared runtime atomically with no leftover staging directories. Follow-up checks before Xvfb teardown found no application/Office subprocesses from that run. No autostart entries changed. |
| Production delivery | Final DEB installed; installed launcher and release RUN hashes matched. Version, desktop entry and production rejection of test-only checks passed. The production administrator UI was opened and inspected on GNOME. |

## Independent Gemini checks

Gemini round one selected and executed unit/media/documents/gui checks; all four commands returned zero. Its report write was blocked by the CLI policy, so that round has execution evidence but no written independent report. Round two selected and executed media/portable checks; both returned zero and an independent report was returned.

Gemini flagged XIO/D-Bus messages at virtual-display shutdown. A subsequent check while Xvfb was still alive confirmed that the application's playback and Office processes had exited. The production administrator desktop log was empty. Test-session portal/D-Bus teardown warnings do not by themselves establish a player crash. Font claims are limited to actual font-selection assertions and separately inspected screenshots, not every glyph.

## Limits

- Only the Ubuntu version and x86_64 environment above were tested; other distributions, ARM, all codecs and all GPU/audio devices remain unverified.
- Audio checks establish decoding, not subjective listening quality. Fixtures do not cover every damaged file or codec combination.
- Complex PPT animations/media and PowerPoint pixel-identical rendering are not established.
- No real system reboot, abrupt power-loss cleanup or reboot-after-install behavior was tested. This edition deliberately has no login/boot autostart.
- Production media/fault automation was not run: those checks used a test build that suppresses system restart. Production checks cover installation, version and desktop launch.
- Original PPT playback requires private temporary plaintext; Linux local state is not DPAPI/TPM-backed. Offline licensing cannot defeat screen recording, privileged inspection or system-state rollback.

Release checksums are attached to [Ubuntu 0.1.0](https://github.com/shuizhaodemumu/Encrypt-your-digital-assets/releases/tag/ubuntu-v0.1.0). Documentation embedded in the existing installers is a build-time snapshot; this source summary includes final installation and follow-up checks.
