# 安卓视频预览版真机验证

日期：2026-10-26。版本：0.1.0-preview。设备：22127RK46C / Android 16 / ARM64 / 4 KiB 页面。

## 已通过

- Rust 核心测试：视频跨块读取、随机读取、EOF、真实设备密钥解封、错误设备、伪造 LIC、过期读取、视频密文篡改、发行口令错误。最后本地运行 1 个集成测试通过（包含多项断言）。
- Gemini 协作桥执行白名单 rust-security-roundtrip：退出码 0、1 项通过、无超时。桥接返回的总结为空，因此仅记录命令实际结果，不将其作为完整安全审计。之后内容 ID 校验和期限运算改动由 Codex 重新运行核心测试通过。
- 手机端导入合成 MP4 加密包，生成加密设备请求；电脑使用真实测试发行密钥签发 LIC；手机通过 Android Keystore 解封并激活。
- LibVLC 通过 Rust 按需解密的代理文件描述符播放，画面居中、按比例适配横屏。
- 12 秒合成视频连续循环多次；保留的 18 次采样全部 playing=true、recoveries=0、error 为空，跨越一次自然回到开头。观察区间未执行 seek。
- 点击画面后控制栏滑出；停止操作 1 秒后开始缩回。采样从 controls_hidden=false、translation_y=0 变为 true、189。
- 非调试 APK 已安装，签名 v2/v3 验证通过。ADB run-as 返回 package not debuggable，符合预期。
- 非调试版本播放窗口包含 SECURE 标记。ADB screencap 得到黑色画面；3 秒 screenrecord 的 98 帧视频通过 blackdetect 检测为黑色画面，未录到合成视频内容。
- 覆盖安装保留本机激活状态，非调试版可从首页再次打开已授权测试视频。

## 测试资料

仅本机保存：work/playback-observation.json、controls-revealed.png、controls-hidden.png、phone-fullscreen-fit.png、release-video-secure.png、release-recording-test.mp4。

Gemini 原始执行记录位于协作目录 reports/encrypt-video-android-2026-10-25T16-44-39-245Z-62f5dfcb-148b-46de-96f3-b6dce02e532b/results.json。不包含发行私钥和本机设备请求正文。

## 尚未覆盖

这是一台设备上的预览验证，不等于全部安卓机型兼容性或第三方安全审计。尚未实测 Android 8–15、16 KiB 页面设备、长时间大文件、所有视频编码/容器、真实到期跨天、Root 注入绕过和 OEM 系统录音策略。仅新增的录音捕获禁止声明未进行音频专项测试，不承诺绝对防录音。

FLAG_SECURE 防护依赖系统执行。Root 修改系统、内存读取、其他设备拍摄等仍可能取得内容；没有执行 Root 命令或手机重启测试。

## 下次检查

手机应用名称为“加密视频 · 安卓预览”。已保留授权过的合成视频，点“全屏播放 · 单个循环”即可检查。调试测试 APK 留在 work/apk，不应分发；对外试用仅用 outputs 中的非调试 APK。

## 2026-10-26 独立管理员 GUI 修复

原 EXE 无参数时退出是命令行入口行为。现在 Windows 上无参数启动会打开独立安卓授权窗口，CLI 参数用法保留。使用和既有两端相同的淡背景、卡片和图像。

实际 Win32 测试通过：无参数启动、口令遮罩、空口令提示、原视频选择与加密、零天数拒绝、设备请求选择与 LIC 签发、中英切换保持期限、弹窗关闭和最小化恢复重绘。使用合成视频，未操作手机；证据见 work/admin-gui-results.json。两项 Rust 测试通过：加密授权往返与期限输入。Gemini 桥接重跑两项白名单检查均退出 0、无超时。
