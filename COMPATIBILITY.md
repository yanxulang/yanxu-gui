# 兼容性

## 版本基线

| 项目 | 言窗 1.0.0 |
| --- | --- |
| 最低言序 | 1.1.12 |
| 言序清单格式 | 2 |
| 言序锁格式 | 2 |
| 原生 ABI | v2 |
| 源码构建 Rust | 1.92，Edition 2024 |
| 包名 | `yanxu-gui` |
| 推荐导入别名 | `言窗` |
| 许可 | MIT OR Apache-2.0 |

1.0.x 不支持言序 1.1.11 或更早版本，也不提供 ABI v1 后端。应用必须使用与运行目标
匹配的锁文件和原生制品。

## 平台目标

| 操作系统 | x86-64 | ARM64 | 窗口后端 |
| --- | --- | --- | --- |
| Linux GNU | `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu` | Wayland、X11 |
| macOS | `x86_64-apple-darwin` | `aarch64-apple-darwin` | Cocoa（经 winit） |
| Windows | `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc` | Win32（经 winit） |

发布包只应登记 CI 在对应原生托管机上实际构建的六个制品。Linux musl、WebAssembly、
iOS、Android 和 32 位目标不属于 1.0.0 支持矩阵。

## 公共 API

1.x 遵循语义化版本：

- 已公开类、函数、方法和稳定错误码不会在 1.x 中删除或改变主要语义；
- 向后兼容的新控件、属性、事件或错误码可以在次版本增加；
- 实现修复、平台兼容和更严格的安全拒绝可以在补丁版本发布；
- 需要重命名、移除或改变调用方式的修改进入新的主版本。

`api/api-v1.json` 是工具读取的 API 快照，`docs/API.md` 是同一快照的阅读版本。两者
必须由当前 `src/主.yx` 生成并保持一致。

## 权限兼容

| 能力 | 清单权限 | 未授权行为 |
| --- | --- | --- |
| 运行窗口事件循环 | `图形界面 = true` | `GUI_PERMISSION` |
| 系统剪贴板 | `剪贴板 = true` | `GUI_PERMISSION_CLIPBOARD` |
| 文件与目录对话框 | `文件对话框 = true` | `GUI_PERMISSION_DIALOG` |

升级不会自动扩大应用权限。只使用窗口和控件的应用可以不授予剪贴板和文件对话框。

## 原生制品兼容

运行时按操作系统、架构、ABI、文件大小和 SHA-256 精确选择制品，不回退到相近架构，
也不从网络下载缺失文件。修改或重新编译动态库后必须重新生成清单和消费锁。发布标签中
同一路径的制品不可静默替换。

源码构建锁定 `eframe/egui 0.35.0`、`winit 0.30.13`、`arboard 3.6.1`、
`image 0.25.10`、`rfd 0.17.2` 和 `sha2 0.10.9`。完整依赖与补丁说明见
[docs/THIRD_PARTY.md](docs/THIRD_PARTY.md)。

## 已知限制

- `应用.运行()`阻塞当前线程，所有资源只能由创建它们的线程和事件循环访问；
- 非零布局`伸缩`在 1.0 中明确返回 `GUI_LAYOUT_FEATURE`；
- 图片只接受受限的 PNG/JPEG；
- 高频鼠标、尺寸和重绘事件可能合并，不适合作为逐物理事件审计流；
- 剪贴板和文件对话框的外观、可用性与取消行为由操作系统决定；
- 不声明跨设备统一 FPS、字体字形或像素完全一致的渲染结果。

资源和性能上限见 [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md) 与
[docs/PERFORMANCE.md](docs/PERFORMANCE.md)。
