# Windows 2.5.1 验证记录

2026-11-04，仅 Windows 产品。本次未修改 Android/Ubuntu，未执行真实重启。

## 实现

`src/packaging.rs` 统一后台转换、分块加密、EXE 写入与源名称规则。GUI 通过 mpsc 接收进度；主线程独占 Win32 控件。管理员禁止预览/硬件 RichEdit，始终按同一双栏坐标布局。

管理员隐藏系统非客户区标题栏，自定义最小化/关闭与顶部拖动命中。角色限定为管理员；内容查看器仍保留原本窗口模式。

`watchdog::enabled_for` 拒绝 presentation/pdf/text；启动、客户端和监督入口均检查。文档启动清除该内容旧 Run 项，`--autostart` 直接返回，不影响用户其他启动项；LIC 和 PowerPoint 安全放映租约保留。

## 本地实测

- cargo test --locked --features watchdog-test-mode：25 通过，1 默认忽略。
- 单独执行真实 VLC 循环测试：通过，4 次循环、1582 样本；输出消失/白边/Ended 样本均 0。授权撤销后读取/跳转仍拒绝。
- 419430809 字节合成 PPTX（400 MiB 填充）GUI 导入、加密：通过；中间进度 28%、75%，最终 100%，WM_NULL 响应观测最大 62 ms。
- 完整解密 SHA-256 与源一致；未播放合成 PPTX，不以此声称实际 Office 内容兼容。
- 实际设备加密请求导入、LIC 签发后：无硬件明文 RichEdit，所有主要控件坐标与初始相同。
- .exe.olmeta 旧侧文件可导入；最小化恢复位置保持；无系统标题栏、自定义关闭正常。
- 文本 EXE 实际授权打开：正常显示；无“修复自启动”按钮；Run 键前后一致；--autostart 直接退出。
- 设备导出保存对话框通过 UI Automation 确认默认“文档.josn”。

## 独立协作与边界

Gemini 桥接选择并实际执行 safe-unit，退出码 0；其计划记录了 GUI、Word 外部转换等覆盖缺口，但最终报告返回为空，不视为完整独立验收。上述 GUI 和 VLC 操作由 Codex 本地执行。

未执行真实重启/登录回归，未逐一测试所有 Word/PPT 内容与 Office 版本；视频策略单元检查和 VLC 实播通过，不等同所有编码兼容。既有内容 EXE 不会自动升级，需要重新打包。
