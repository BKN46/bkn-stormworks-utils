# Stormworks video_get 最小源码单元

本目录是从分析工作台复制出的最小可编译源码，只包含：

- `video_get_plugin`：生成 `StormworksVideoGet.dll`。
- `openal64_proxy`：生成 replace-DLL 加载代理 `OpenAL64.dll`。
- `shared`：插件使用的公共配置、JSON、PE 和哈希代码。

本目录不包含 mod manager、GUI、Cargo 构建缓存、游戏文件、安装包、日志、崩溃转储或逆向分析 artifacts。

## 环境

- Windows x64
- Rust stable toolchain
- Cargo

## 编译

在本目录执行：

```powershell
cargo build --release -p stormworks_video_get -p stormworks_openal64_proxy
```

输出文件：

```text
target\release\StormworksVideoGet.dll
target\release\OpenAL64.dll
```

运行测试：

```powershell
cargo test -p stormworks_video_get -p stormworks_openal64_proxy
```

## 目录

```text
source_minimal\
  Cargo.toml
  Cargo.lock
  README.md
  .gitignore
  shared\
  video_get_plugin\
  openal64_proxy\
```

## 重要说明

- 这是源码编译单元，不是可直接安装的完整发布包。
- 不包含 Stormworks 原始 `OpenAL64.dll`、`OpenAL64_real.dll` 或 `stormworks64.exe`。
- 实际 Hook 地址、目标字节、运行配置和安装脚本需要由外部打包层提供。
- 当前 Hook 实现与特定 Stormworks 版本相关，不能假定兼容其他游戏版本。
- 源码中仍有本地默认路径和研究阶段代码，公开发布前应继续清理。
- 当前 Cargo 许可证字段为 `UNLICENSED`。公开分享前必须由项目所有者选择许可证并添加对应 `LICENSE` 文件。
