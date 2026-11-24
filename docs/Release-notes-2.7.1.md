# 联网正式版 2.7.1

Windows 工作台和 NAS 在线管理后台同步更新。Windows 用户下载 x64 ZIP，NAS 用户下载 Docker ZIP。建议两端一起升级。

- Windows 内容程序只在手动打开时运行。已分发的旧 EXE 需替换为升级后的内容程序；手动打开新版一次即可清理该内容的旧登录启动项。
- 工作台打包前需选组并保存，或手动选择离线模式。联网打包的内容登记到所属组，后台切换页签时刷新列表。
- NAS 可删除空组。已有内容的组可暂停；首次联网激活后，设备与授权页显示内容、设备标识、到期时间与剩余期限。新版 Windows 内容程序还会报告电脑名称。
- 升级 NAS 时沿用原 Compose 项目名和 license-data 数据卷。公开工作台首次连接时填写自己的 HTTPS 服务地址。

[部署与使用说明](https://github.com/shuizhaodemumu/Encrypt-your-digital-assets/blob/windows-online-v2.7.1/docs/WINDOWS-ONLINE.zh-CN.md)
