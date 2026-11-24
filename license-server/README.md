# Windows 联网正式版 2.7.1

本版同时提供 Windows x64 签发工作台和可自行部署的 NAS 在线管理后台。Windows 程序包内运行 Packager.exe。DeviceTool.exe 用于设备请求。自托管用户首次连接时填写自己的 HTTPS 服务地址；定制构建可在编译前设置 LICENSE_MANAGER_DEFAULT_URL。

- Windows 内容程序只在手动打开时运行。已分发的旧 EXE 需替换为升级后的内容程序；手动打开新版一次即可清理该内容的旧登录启动项。
- 工作台打包前需选组并保存，或手动选择离线模式。联网打包的内容登记到所属组，后台切换页签时刷新列表。
- NAS 可删除空组。已有内容的组可暂停；首次联网激活后，设备与授权页显示内容、设备标识、到期时间与剩余期限。新版 Windows 内容程序还会报告电脑名称。
- 升级 NAS 时沿用原 Compose 项目名和 license-data 数据卷。公开工作台首次连接时填写自己的 HTTPS 服务地址。

## 部署与领取令牌

下载并解压 Docker 部署包，进入 license-server 目录，在 Docker 主机执行：

```sh
docker compose up -d --build
curl -f http://127.0.0.1:7187/health
docker compose exec license-manager assets-license-server --today-token
```

首次构建需要联网下载基础镜像和 Rust 依赖。用反向代理将有效 HTTPS 域名转发到同一主机的 127.0.0.1:7187。浏览器访问 HTTPS 根地址，用当天令牌登录。北京时间每天 04:00 换发；过期后在容器内重新领取。数据库和签名密钥保存在 license-data 卷，请一并备份。升级已有部署时沿用原 Compose 项目名称和数据卷。

## 授权流程

1. 在 NAS 后台创建组。在 win授权工作台连接管理器，填写当天令牌，刷新并选择组。
2. 使用发行者密钥打包内容，导入收件人设备请求并签发 LIC。客户端本地校验 LIC 后向服务器取得签名短期许可。
3. 后台支持按组、内容或设备授权暂停和恢复。正常约 30 秒续期，断网时仅可运行到本次许可到期（最长 120 秒）。旧离线内容需重新打包再签发才能加入管理。
4. 管理员令牌仅在管理端使用。公开部署包内不含运行数据库、发行者私钥或管理员凭据。

## 从源码构建

Windows 使用 Rust stable，在仓库根目录运行 `cargo build --release --locked`。服务端入口位于 license-server，协议位于 online-license。自行构建 Docker 使用上面的 Compose 命令。本项目源码按 MIT 开源，捆绑第三方组件按各自许可证分发。
