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
