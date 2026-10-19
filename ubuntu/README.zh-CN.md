# Encrypt your digital assets · Ubuntu 版

Rust 离线内容加密、设备授权和查看工具。独立于上级 Windows 2.4.3，**不创建开机或登录自动启动项**。

[English](README.md) · **Ubuntu 0.1.0** · [下载安装包](https://github.com/shuizhaodemumu/Encrypt-your-digital-assets/releases/tag/ubuntu-v0.1.0)

![Ubuntu 授权工作台](docs/workspace-zh.png)

独立目录保存源码、构建脚本与验证说明。详见 [安装与使用](docs/安装与使用.md) 和 [验证记录](VALIDATION.md)。

## 给使用者

在上述 Release 中选择一种安装包；本地源码构建的产物位于 `outputs/`：

- `encrypt-assets-ubuntu_0.1.0_amd64.deb`：Ubuntu 桌面安装包。安装后从应用菜单打开 **Encrypt your digital assets**。
- `Encrypt-assets-Ubuntu-0.1.0-release-x86_64.run`：离线安装与便携包。可在终端执行 `sh 文件名.run`，首次运行解压到当前用户目录，并建立应用菜单入口。

程序自带 GTK、LibVLC、Poppler、LibreOffice Impress/Writer、Python/UNO、中文字体和配套动态库，用户不需要安装 Rust、Cargo、Docker 或手工配置播放器。安装包约 370 MiB，首次运行解压到用户目录，建议预留至少 2 GiB 空间。

这是 **Linux x86_64 桌面软件**。Windows 和 ARM 电脑不能直接运行。打包内的私有动态加载器用于减少系统库版本依赖；其他 Ubuntu 版本的兼容性须以验证记录为准，不能据此宣称所有 Linux 系统均可运行。

### 管理员流程

1. 新建并保存有口令保护的发行密钥，或导入此前的密钥备份。
2. 选择原文件，点击“生成加密内容程序”。安装版生成单个 `.run` 文件，内部包含运行库和加密内容；开发版可生成依赖系统库的 `.bin`。
3. 将内容程序发给接收方。`.olmeta` 管理员记录和 `.issuerkey` 私钥备份由管理员保管，不需要发给接收方。
4. 接收方打开内容程序，点击“获取本机设备识别码”，导出加密 `.json`。
5. 管理员选择对应内容程序和 `.json`，设置授权天数、首次导入窗口（秒/分钟/小时/天），确认后签发 `.lic`。
6. 接收方导入 `.lic` 后才能播放或查看。以后手动打开同一个程序时，仍在有效期内的授权会恢复。

首次导入窗口从**签发时刻**开始；使用天数从**第一次成功激活**开始。重复导入同一 LIC 不会重置使用期限。取消确认不会签发授权。

### 文件类型

| 内容 | 实现 |
| --- | --- |
| MP4、MKV、AVI、MOV、WEBM | LibVLC 从加密数据读取回调中按块解密；默认全屏、隐藏控制栏、单视频循环；同一进程持续播放 |
| MP3、WAV、M4A、FLAC、OGG | LibVLC 音频播放，可暂停、跳转、调节音量 |
| PDF | Poppler 内存读取、内部翻页；不启动外部编辑器 |
| TXT | 内存解密后只读显示，UTF-8/GBK 回退 |
| PNG、JPEG、GIF、BMP、TIFF | GTK 图像解码后显示；不承诺所有动图均保持动画 |
| DOC、DOCX、ODT | 管理员端通过内置 LibreOffice Writer 转换为 PDF，再加密打包 |
| PPT、PPTX、PPS、PPSX、ODP | 原文件加密；授权后通过 LibreOffice Impress UNO 隐藏只读文档、直接启动放映。不会转换为静态 PDF |

视频快捷键：空格暂停/继续，左右键跳转 10 秒，F11 切换全屏，Esc 退出全屏，移动鼠标显示控制栏。

PPT 的排版、动画、字体和嵌入媒体受 LibreOffice 兼容性影响，不能保证与 Microsoft PowerPoint 完全相同。宏禁用，外部链接更新禁用。放映时原文件临时写入当前用户的私有运行目录，正常关闭后清理；系统突然断电等情况不等同于完成清理。

## 加密与设备绑定

- 继承 Windows 版本的分块格式：1 MiB AES-256-GCM，随机内容密钥，头部和块序号作为认证数据。
- LIC 使用 ECDSA P-256 签名；内容密钥用接收设备 RSA-3072 公钥通过 OAEP/SHA-256 包装。
- 设备文件使用临时 P-256 ECDH、HKDF-SHA-256 和 AES-GCM 加密，只能由对应发行者解密。
- Linux 硬件采集来自 `/sys` 与 `/proc`，包括可读取的电脑/主板、CPU、磁盘、内存槽位和物理网卡。**不收集公网 IP、内网 IP、DNS 或位置**。
- 稳定绑定排除 USB/可移动磁盘和网卡连接状态。Linux 权限不允许读取的序列号留空。读取不到 DMI UUID 时明确使用 `/etc/machine-id` 作为系统安装标识回退；重装系统或改变虚拟机配置可能需要重新授权。
- Linux 使用独立用户数据目录：目录权限 0700，密钥/授权文件权限 0600，本地状态使用随机主密钥加密。主密钥也保存在该用户私有目录；这**不是 Windows DPAPI 或 TPM 保护**，无法抵御拥有当前用户或 root 权限的人读取它。
- Windows 的可执行文件不转换为 Linux 程序；Linux 内容需要在本版重新打包、导出设备请求和签发授权。私钥备份格式沿用，但不宣称 Windows/Ubuntu 的设备绑定和旧 LIC 通用。

离线授权不是绝对 DRM。屏幕录制、系统管理员读取内存、二进制修改和整个系统状态回滚不能由本软件完全阻止。离线环境也没有可信服务器时间或远程吊销。

## 运行监测

内容程序的父进程监测查看器心跳、解码错误和播放进度。正常结束、暂停、手动关闭与授权到期不算故障。故障时重启该内容程序；10 分钟内第 3 次故障重启请求系统立即重启。Linux 系统必须允许当前会话执行重启；拒绝时停止重试并输出错误。

所有自动化测试均开启 `watchdog-test-mode`，第三次故障只报告 `TEST_ONLY_REBOOT_SUPPRESSED`，不重启电脑。没有创建 XDG autostart、systemd 登录自启或开机任务。

## 开发者构建

在 Ubuntu x86_64 开发环境安装以下工具。此步骤仅适用于构建源码，不是交付给使用者的环境配置要求。

```sh
sudo apt-get install build-essential pkg-config rustc cargo rustfmt \
  libgtk-4-dev libvlc-dev vlc-plugin-base vlc-plugin-video-output \
  libpoppler-glib-dev libreoffice-impress libreoffice-writer python3-uno \
  fonts-noto-cjk ffmpeg xvfb xdotool python3-pyatspi patchelf \
  desktop-file-utils bubblewrap dbus-x11
```

```sh
cargo fmt --check
ENCRYPT_ASSETS_DATA_DIR="$PWD/work/unit-state" cargo test --locked --features watchdog-test-mode -- --test-threads=1
cargo build --locked --release --features watchdog-test-mode
xvfb-run -a dbus-run-session -- target/release/encrypt-assets-ubuntu --self-check
xvfb-run -a dbus-run-session -- python3 tests/integration.py target/release/encrypt-assets-ubuntu
python3 build/bundle.py target/release/encrypt-assets-ubuntu --suffix test
xvfb-run -a dbus-run-session -- python3 tests/portable.py outputs/Encrypt-assets-Ubuntu-0.1.0-test-x86_64.run
# 完成测试后构建正式程序（不带测试特性）
cargo build --locked --release
python3 build/bundle.py target/release/encrypt-assets-ubuntu --suffix release
```

源码结构：`core.rs` 加密/包格式/签名，`hardware.rs` 加密设备请求，`linux_hardware.rs` Linux 采集，`platform.rs` 用户状态与单实例锁，`session.rs` 授权期限，`media.rs` LibVLC 内存回调，`documents.rs` PDF 与 Impress 宿主，`gui.rs` GTK4 交互，`watchdog.rs` 故障监测，`build/bundle.py` 离线运行库与 DEB 打包。

原始美术沿用已有白发淡紫龙娘素材，来源记录在 `docs/artwork-provenance.json`，本次没有宣称重新调用 H3 生成图片。

随包运行库的版权声明位于解压目录的 `docs/third-party`，组件来源清单位于 `docs/runtime-manifest.json`。打包器仅调整 LibreOffice 的发行版固定路径、私有 ELF 解释器与库搜索路径；`uno.py` 保留上游源代码和版权声明，并注明路径调整。运行时不下载依赖。

实现参考：[GTK4 Rust 文档](https://gtk-rs.org/gtk4-rs/stable/latest/book/installation_linux.html)、[LibVLC 3 输入回调](https://videolan.videolan.me/vlc-3.0/libvlc__media_8h.html)、[LibreOffice 演示接口](https://api.libreoffice.org/docs/idl/ref/interfacecom_1_1sun_1_1star_1_1presentation_1_1XPresentation2.html)、[LibreOffice 宏禁用常量](https://api.libreoffice.org/docs/idl/ref/namespacecom_1_1sun_1_1star_1_1document_1_1MacroExecMode.html)。
