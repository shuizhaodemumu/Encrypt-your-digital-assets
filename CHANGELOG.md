# Windows 2.6.0 · EXE 与 DLL 整目录授权

发布日期：2026-11-14。离线稳定版。

## 新增

- 加密主 EXE 所在的程序文件夹，保留 DLL、配置、资源、子目录和空目录；通过设备绑定 LIC 验证后启动。
- EXE 类内容不启用看门狗、故障重启、强制全屏或开机自启动。
- 独立临时会话及 Windows Job Object 管理运行进程；正常退出清理，关闭授权窗口或授权失效时结束该会话进程树。
- 输出包必须放在源程序目录之外；拒绝越界、重复和链接条目，排除发行密钥及管理员侧文件。

## 下载与使用

下载 `Encrypt-your-digital-assets-2.6.0-windows-x64.zip`，解压后运行 `Packager.exe`。`DeviceTool.cmd` 打开设备识别码工具，`Install.cmd` 安装当前用户入口。随包提供中英文 README、许可和第三方声明、验证记录；`SHA256SUMS.txt` 用于校验下载文件。

将主 EXE 和依赖整理到独立文件夹，导入主 EXE并确认范围，然后在文件夹外生成授权 EXE。接收者导出设备请求，管理员签发 `.lic`，接收者导入后运行。`.olmeta`、`.josn`、`.lic` 默认沿用源文件名主体。

## 升级与边界

2.6.0 不包含 NAS 联网监管预览。Windows 2.7 预览、Ubuntu 和 Android 版本本次均未发布更新。保留发行密钥、口令及管理员记录；旧内容程序内嵌旧代码，应用新行为需重新打包并签发匹配 LIC。

程序运行时有临时明文，不能防御本机管理员提取；程序目录中的新增数据会随会话清理，重要结果请保存到外部目录。系统 .NET/VC++、服务、驱动及 COM 依赖不会自动安装。PowerPoint 原生放映仍需桌面 PowerPoint，Word 转换仍需管理员侧 LibreOffice。

## 验证

29 项常规测试通过，3 项桌面/Office 项默认忽略。实际 EXE+DLL 验证通过：未授权不启动、授权加载依赖、无新增自启动项、子进程生命周期、正常退出不重启、关闭包装器结束进程树并清理。没有进行真实系统重启或任意第三方程序兼容性保证。

## English

Windows 2.6.0 adds encrypted application-folder packaging (main EXE, DLLs, configuration and resources) with offline, device-bound LIC authorization. Application content has no watchdog, forced fullscreen or login autostart. A temporary session and Windows Job Object bound process lifetime and cleanup. Extract the Windows ZIP and run Packager.exe. See the included READMEs for dependencies, upgrade steps, temporary-data behavior and protection limits. This is the offline stable release, not the 2.7 online preview.

---

# Windows 2.5.2 · PowerPoint 受控放映与后台打包

发布：2026-11-05（本地功能验证：2026-11-04）。本次合并尚未公开发布的 2.5.1 修复；Android、Ubuntu 版本不变。

## 修复内容

- 拦截受保护 PPT 的常规保存与另存为，调整退出顺序，丢弃修改和墨迹批注；其他文稿保持正常保存。
- 独占会话锁保护正在使用的临时目录，退出清理明文，下一次启动回收异常退出残留。
- PPT、PDF、Word 转换内容、TXT 不注册登录自启动或重启看门狗；视频/音频与图片保留原策略。
- 管理员隐藏白色系统标题栏，保持双栏工作台，不再导入预览或显示硬件明文报告。
- Word 转换、内容加密和 EXE 写入在后台执行并显示进度，避免大文件阻塞窗口。
- 源文件名贯穿侧文件：`逐梦.pptx` → `逐梦.olmeta` / `逐梦.josn` / `逐梦.lic`；兼容旧 `.exe.olmeta` 和 `.json`。

## 下载与升级

解压 `Encrypt-your-digital-assets-2.5.2-windows-x64.zip`，打开 `Packager.exe`。`DeviceTool.cmd` 打开设备请求工具，`Install.cmd` 安装本机入口。运行库随程序嵌入；PowerPoint 与管理员端 Word 转换所需的 LibreOffice 仍需另行安装。

**旧 EXE 内嵌旧代码，需要重新打包并按新内容包重新签发授权，不能只替换管理员后继续使用旧内容 EXE。** 保留发行者密钥、口令和管理员记录；不要清空共享设备状态目录。

## 验证与限制

26 项常规测试通过；默认忽略的 PowerPoint 桌面测试单独执行：Save/SaveAs 拦截、其他文稿正常保存、四种带墨迹退出路径通过。实际打包 EXE 的 LIC 签发/导入、放映、另存拦截、退出明文清理通过。400 MB PPTX 后台加密及完整解密 SHA-256 验证通过。

这是受控放映加固，不是绝对防复制的 DRM。PowerPoint 放映期间仍需明文临时文件；不能保证抵御本机管理员、内存提取、所有 Office 导出 API 或自动恢复副本。墨迹退出提示处理适配已实测中文 Office，并匹配英文文案；其他语言/版本需额外验证。未执行真实系统重启测试。

Gemini 桥接执行安全单元测试成功；其最终报告正文为空，未将其表述为完整独立验收。完整细节见 [验证说明](docs/Windows-2.5.2-validation.md)。

## English

Windows 2.5.2 hardens native PowerPoint playback, blocks ordinary Save/Save As for the protected presentation, discards edits/ink on exit, and cleans abandoned application temp sessions using exclusive locks. Documents no longer enable startup/watchdog recovery; media policy remains. Background packaging keeps the caption-free two-column administrator responsive. Sidecar/enrollment/license defaults follow the source stem. Repackage old content and issue new matching licenses. This is not DRM against OS administrators or all Office extraction paths. See the bilingual README and validation record for dependencies and limits.
