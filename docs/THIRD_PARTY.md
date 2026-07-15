# 第三方依赖与许可

言窗原生后端使用下列直接依赖。版本以 `Cargo.lock` 为准；发布构建必须使用
`--locked`，并通过 `cargo audit --file yanxu-gui/Cargo.lock` 安全审计。

| 依赖 | 用途 | 许可 |
| --- | --- | --- |
| `eframe` / `egui` 0.35.0 | 窗口、控件、布局、绘制与可访问性 | MIT OR Apache-2.0 |
| `winit` 0.30.13（间接） | Windows、macOS、Wayland、X11 事件循环 | Apache-2.0 |
| `arboard` 3.6.1 | 系统剪贴板 | MIT OR Apache-2.0 |
| `image` 0.25.10 | PNG、JPEG 图片解码 | MIT OR Apache-2.0 |
| `rfd` 0.17.2 | 原生文件选择对话框 | MIT |
| `sha2` 0.10.9 | 资源摘要校验 | MIT OR Apache-2.0 |

## Wayland 安全补丁来源

crates.io 上的 `wayland-scanner 0.31.10` 依赖存在安全公告的
`quick-xml 0.39`。工作区将同版本包固定到 Smithay 官方 `wayland-rs` 提交
`249f35c8ce18c8a8000c627ffe04ac235bc0764f`；该提交改用
`quick-xml 0.41`，许可仍为 MIT。固定完整提交号保证构建可复现，也避免跟随浮动
分支。上游来源与固定值同时记录在根 `Cargo.toml` 和 `Cargo.lock` 中。

## 发布核对

- 项目自身许可全文位于 `LICENSE-MIT` 与 `LICENSE-APACHE`。
- 新增或升级依赖时，必须重新核对许可证并更新本表。
- 发布前必须运行 `cargo audit`，且不得忽略高危公告。
- 最终分发包应保留本说明、项目许可和依赖要求的声明。
