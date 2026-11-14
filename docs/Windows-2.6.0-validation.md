# Windows 2.6.0 验证记录 / Validation

发布验证：2026-11-14。2.6.0 为离线稳定版，发布源码与此次发布二进制在独立目录构建；未混入 2.7 联网预览功能。

- Rust 常规测试：29 passed，0 failed，3 ignored。忽略项依赖实际 Office/桌面媒体环境，本次没有重跑所有历史 Office/视频场景。
- 使用发布构建运行实际 EXE + DLL：未授权不启动；导入 LIC 后成功读取 DLL、嵌套配置和空目录；父进程退出后保留仍运行的子进程；全部正常退出不重启并清理；再次手动启动成功；关闭授权窗口结束其主/子进程并清理。
- 测试前后 Windows 当前用户 Run 项一致，没有新增开机自启动。未进行真实系统重启。
- 目录打包单元测试覆盖依赖/空目录保留以及不安全路径的拒绝。
- 发行包为正常 release 构建，不启用 watchdog-test-mode。EXE 类内容自身不启用 watchdog；视频等既有策略仍受相应规则约束。
- 媒体、PowerPoint、大文件后台加密等历史验证见 [2.5.2](Windows-2.5.2-validation.md)，不表述为本次对所有旧功能重新验收。

## 限制

运行时需要在本机临时目录恢复可执行文件。能够控制操作系统或进程的人仍可能提取明文。应用写入自身目录的数据会随会话清理；重要输出须另存外部目录。没有承诺任意第三方 EXE、反作弊程序、服务、驱动或提权程序兼容。系统运行库仍由使用者安装。

## English

The release-source build passed 29 normal Rust tests; three desktop/Office-dependent tests remain ignored. A harmless real EXE plus DLL passed licensing, dependency/resource loading, child-process lifetime, normal cleanup without relaunch, manual relaunch and owned-process-tree termination on wrapper close. Current-user Run registrations did not change. No real reboot was performed. Prior media/Office validation is historical, not a new full regression claim.
