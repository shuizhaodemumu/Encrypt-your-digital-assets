# Windows online verification 2.7.0-preview.1 / Windows 联网验证版

仅发布 Windows 联网客户端工具和源码；NAS 服务端、Ubuntu、Android 联网版本不在本次发布范围。需要已有兼容授权服务。

- 保留设备绑定 LIC，增加 HTTPS 服务器签名的短期许可。
- 支持创建/选择组，联网模式选组后才能打包。
- 管理端可按组、内容、设备授权撤销/恢复；约 30 秒续期，许可最长 120 秒，断网到期停止。
- 支持媒体、Office 文档及附带 DLL 的 Windows EXE 文件夹；旧离线内容须重新打包才能接受监管。
- Windows 29 tests passed / 3 interactive tests skipped; shared protocol 2 tests passed. Release binary rebuilt from branch source.

下载 Windows ZIP，解压运行 Packager.exe。请先备份密钥；与稳定 Windows 版共享本机配置目录。SHA256SUMS.txt 提供校验值。此预发布不替换稳定版 2.6.0。

This release contains Windows tools and source only. An existing compatible licensing server is required. No server deployment bundle, Linux or Android online build is included.

[完整使用说明](https://github.com/shuizhaodemumu/Encrypt-your-digital-assets/blob/windows-online-v2.7.0-preview.1/docs/WINDOWS-ONLINE.zh-CN.md)
