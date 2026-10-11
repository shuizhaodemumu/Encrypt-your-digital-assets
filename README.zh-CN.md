# 音视频与文档离线加密授权 · Rust

中文 | [English](README.md)

Windows 桌面软件，将音频、视频及文档加密打包为独立 EXE，通过设备绑定和离线 LIC 授权控制使用。**2.2.0** 在 Rust 修复 Office 版 2.1.1 的基础上加入中英文界面。

[下载 Windows x64 成品程序包](https://github.com/shuizhaodemumu/offline-media-license-rust/releases/tag/v2.2.0)，解压 ZIP 后运行 `Packager.exe`，使用成品无需安装 Rust。包内同时提供设备工具、安装入口和已有内容升级工具。

## 主要功能

- **音视频加密**：分块 AES-256-GCM 加密，LibVLC 通过受控解密流读取媒体；视频授权后无边框全屏、隐藏控件、单个循环播放，到期停止。循环改为引擎内部回放，保持视频输出窗口；播放画布与全屏底色改为黑色，减少结束后重播时的闪白和停顿。
- **离线授权**：加密设备报告、发行者签名、硬件绑定、首次激活 60 秒窗口、发行者密钥加密备份和 Windows DPAPI 本地状态保护。
- **文档查看**：内置 PDF、文本与图片查看；Word DOC/DOCX 由管理员端另行安装的 LibreOffice 转换为静态页面。
- **PPT 原生放映**：原 PPT/PPTX/PPS/PPSX 文件直接加密。接收端需安装并激活 Microsoft PowerPoint 桌面版；运行时使用临时解密文件，只读放映不等于绝对防提取。
- **中英文切换**：管理员、设备工具、安装入口和接收端窗口顶部提供“中文 / English”菜单，记住当前 Windows 用户的选择。切换不重启播放，不改动文件内容、密钥或授权；系统文件选择器按钮语言由 Windows 决定。视频全屏播放时隐藏菜单，恢复授权窗口后重新显示。

## 使用流程

1. 管理员创建发行者密钥并加密备份。
2. 导入并预览文件，生成内容 EXE，保留 `.olmeta` 管理员记录。
3. 接收端运行内容 EXE，导出加密设备报告并交给管理员。
4. 管理员导入报告，设置授权天数，签发 LIC。
5. 接收端在签发后的 60 秒内导入 LIC，激活后开始计算使用期限。

双语版本以 `%LOCALAPPDATA%/OfflineLicenseBilingualRust` 独立保存安装和状态，保留已有 Office 版本。为双语版本签发授权时，使用双语版本生成的设备报告。

## 构建与程序

Windows x64 安装稳定版 Rust 与 MSVC 或 GNU 构建工具后，在仓库根目录运行：

```powershell
cargo test --locked
powershell -NoProfile -ExecutionPolicy Bypass -File build/build.ps1 -Release
```

输出在 `程序/`：`Packager.exe`（管理员）、`DeviceTool.exe`（设备工具）、`Packager-Setup.exe`（安装入口）。构建需要仓库中的 `assets/runtime.zip`；运行库许可、上游源码地址见[第三方说明](第三方说明.md)与 `third-party-licenses/`。

UI 回归使用 `watchdog-test-mode` 构建，仅模拟重启策略，不执行真实重启。生产发布不启用该功能。密钥、真实设备报告、用户文档、授权和本机测试产物不进入源码仓库。

## 升级已有 Rust Office 内容 EXE

内容 EXE 内嵌播放器，因此只替换管理员程序不会修复已经生成的 EXE。使用随附“升级已有内容.cmd”，选择原文件，另存为新 EXE；原文件保留。升级会验证密文不变，复制 `.olmeta`，不延长或替换授权。

该工具专门以原 `OfflineLicenseOfficeRust` 状态目录兼容 Rust 修复 Office 版 2.1.1。先关闭旧内容程序，再打开新文件；不用于 C# 版或协议版本1的旧内容。独立双语版仍使用自己的状态目录。

开发者构建升级工具：`build/build.ps1 -Release -OfficeCompat`。将 `build/Upgrade-Content.ps1` 与 `build/升级已有内容.cmd` 放到 `ViewerUpdater-OfficeCompat.exe` 旁。

专项循环回归：`build/loop-test.ps1 -Fixture 短视频.mp4`，检测4次循环、输出窗口保持、边缘颜色采样和撤销授权后的读取限制。该项需 Windows 桌面和短视频，普通 `cargo test` 会跳过；测试用临时加密包和内存授权夹具，不采集硬件、不安装查看器、不登记启动项。

验收与边界见 [VALIDATION.md](VALIDATION.md)。本次语言修改不代表其他 Office 版本、Word 转换和全新电脑均已实测。
