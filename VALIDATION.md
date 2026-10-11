# Validation / 验收范围

Updated / 更新：2026-10-11 (Asia/Shanghai)

Codex local results / 珑儿本机结果：

- 18 normal Rust tests passed; the desktop-dependent video test is separately marked ignored in the default run. / 18项常规Rust测试通过，普通测试跳过需桌面的专项视频用例。
- Actual UI: administrator/device/setup/recipient activation windows switch Chinese/English, remember language, retain numeric input and user document text; English password/file dialogs and validation messages verified. / 实际窗口的双语、语言记忆、输入及文档保持，以及英语弹窗和验证错误通过。
- Encrypted MP4 native-loop test: four boundaries, 1,583 samples, zero output-window disappearances, zero sampled white edges, zero Ended-state samples; output handles retained. Revoked in-memory authorization blocks read and seek. / 加密MP4观察到4次循环，1583次采样中输出窗口未消失、边缘未采到白色、未进入Ended状态；撤销内存授权后读取和跳转仍被阻止。
- Office-compatible updater: encrypted payload hash, administrator-record hash, decrypted-content hash and original EXE hash preserved on a synthetic fixture. / Office兼容升级器对合成样例保持密文、管理员记录、解密内容和原EXE不变。

Gemini independently executed the four registered checks: 18 unit tests passed (1 desktop test ignored in that run); bilingual UI passed; the separate native-loop test passed with 4 loops / 1,595 samples / 0 output disappearances / 0 sampled white edges / 0 Ended samples; upgrade preservation passed. After the Esc cleanup and window-style adjustment, Gemini reran bilingual UI (including encrypted-video preview / clean Esc exit) and upgrade preservation: both passed. The full-context review found no remaining blocking issue in the tested changes. These checks do not reboot the computer.

樱酱已独立执行四项登记检查：常规18项通过（该轮另1项桌面用例跳过）、双语UI通过、单独的原生循环通过（4次循环 / 1595采样 / 输出未消失 / 未采到边缘白色 / 无Ended）、升级内容保持通过。补充Esc退出清理与窗口样式后，再次独立运行双语UI（含加密视频预览后Esc正常退出）与升级保持，两项均通过；完整上下文复核未保留阻断项。检查不重启电脑。

VLC still emitted imem module-probing and Windows taskbar-thumbnail diagnostics in the native test. The passing assertion covers playback/window continuity, not an absence of diagnostic messages. / 原生测试仍有VLC模块探测及Windows任务栏缩略图诊断日志；验收通过指播放与窗口连续性断言通过，不代表日志完全无诊断消息。

Loop sampling verifies window continuity and sampled borders on one local clip; it does not measure frame-perfect transitions for every codec or remove silence/black frames already present in a source video. The native test uses an in-memory session and a separate test window, not real hardware enrollment or a newly licensed production fullscreen EXE. Real Office playback, fresh installations, other PCs and actual user media are not revalidated by this round.

循环采样验证单一本机视频的窗口连续性和边缘颜色，不代表所有编码逐帧无缝，也不能去除原素材自带的黑帧或静音。专项测试使用内存授权和独立测试窗口，不是真实设备注册或新授权的生产全屏EXE。真实Office、全新安装、其他电脑和用户实际片源未在本轮重新验收。

The 2.1.1 baseline has local Microsoft PowerPoint 16.0 playback evidence. This does not cover every Office version, all user presentations, Word conversion, fresh installations or new machines. Temporary decrypted presentation files remain necessary for external PowerPoint playback.

2.1.1 基线已有本机 Microsoft PowerPoint 16.0 放映证据，不代表其他 Office 版本、所有用户文稿、Word 转换、全新安装或其他电脑均通过验收。外部 PowerPoint 放映仍需要临时解密文件。
