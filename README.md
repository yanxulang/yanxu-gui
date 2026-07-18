# 言窗（yanxu-gui）

言窗是言序的稳定桌面 GUI 包。1.0.0 使用中文对象 API，通过 ABI v2 封装
eframe/egui 与 winit，支持 Windows、macOS、Linux Wayland/X11 的 x86-64 和
ARM64。应用无需编写 Rust，也不会接触原生指针或框架类型。

## 主要能力

- 应用生命周期、亮色/暗色主题、多窗口、全屏、置顶、DPI 和尺寸约束；
- 纵向、横向、网格、层叠、双向滚动布局及递归子布局；
- 文字、按钮、输入、选择、列表、标签页、菜单、图片、进度、滑块和画布；
- 键盘、鼠标、焦点、变化、窗口、拖放、自定义事件和有界定时器；
- 中文与扩展 Unicode 字体回退、平台 IME、工具提示和可访问描述；
- 剪贴板与原生文件/目录对话框，各自独立授权；
- 结构化错误详情、资源快照、确定性清理和跨线程访问拒绝。

## 安装

需要言序 1.1.12 和言包 0.5.0。新建应用时让言包生成依赖、权限和应用元数据：

```sh
yanbao new 我的窗口 --gui
yanbao run --manifest-path 我的窗口
```

既有项目可在`言序.toml`中锁定稳定版本：

```toml
[依赖]
言窗 = { 包 = "yanxu-gui", git = "https://github.com/yanxulang/yanxu-gui.git", 修订 = "v1.0.0", 版 = "^1.0" }

[权限]
图形界面 = true
剪贴板 = false
文件对话框 = false
```

随后运行`yanbao install --manifest-path <项目>`生成或验证`言序.lock`。运行时按锁中
的操作系统、架构、ABI、大小和 SHA-256 精确选择后端，不下载缺失动态库。

## 五分钟示例

```yanxu
引「包:言窗」为 界面；

定 应用 为 界面.应用（「任务清单」）；
定 窗口 为 应用.窗口（{「标题」：「任务清单」，「宽」：720，「高」：480}）；
定 布局 为 窗口.纵向布局（{「间距」：12，「内边距」：16}）；
定 提示 为 布局.文字（「尚未保存」）；
定 输入 为 布局.输入框（{「占位」：「输入任务」}）；
定 保存 为 布局.按钮（「保存」）；

法 保存任务（事件）：空 则
    提示.内容（「已保存：」 加 输入.取内容（））；
    归 空；
终

法 关闭应用（事件）：空 则
    应用.退出（）；
    归 空；
终

保存.点击（保存任务）；
窗口.关闭时（关闭应用）；
窗口.显示（）；
应用.运行（）；
```

`应用.运行()`阻塞当前线程直至退出。关闭窗口、布局或应用会释放子资源和回调；已关闭
句柄不能再次使用。

## 错误处理

原生后端使用稳定 `GUI_*` 代码。不要解析错误消息，也不要直接把 ABI v2 的源代码
`NATIVE_V2`当成业务分类：

```yanxu
试 则
    窗口.标题（「新标题」）；
救 所误 则
    定 详情 为 界面.错误详情（所误）；
    若 （详情【「代码」】 等于 「GUI_RESOURCE_CLOSED」）则
        言「窗口已经关闭」；
    终
终
```

所有代码和恢复边界见 [错误参考](docs/ERRORS.md)。

## 权限

`图形界面`只允许运行窗口事件循环，不隐式授予剪贴板或文件对话框。只使用普通控件的
应用应保持后两项为`false`；实际调用相应功能时再显式开启。言窗不会自行读取文件对话
框返回的路径。

## 构建 Bundle

```sh
yanbao build --manifest-path 我的窗口 --release --bundle
```

产物是 macOS `.app`、Windows GUI 应用目录或 Linux AppDir，包含 standalone 运行
时、YXB、当前目标原生库、资源、许可证和逐文件摘要。签名与公证在未签名摘要验证后由
发布环境完成。

## 兼容性与已知限制

- 最低言序 1.1.12，只支持清单/锁格式 2 和 ABI v2；
- 支持 Linux GNU、macOS、Windows 的 x86-64/ARM64，不支持 musl、WebAssembly、
  移动平台或 32 位目标；
- 非零布局`伸缩`在 1.0 中明确失败；图片只支持受限 PNG/JPEG；
- 高频鼠标、尺寸和重绘事件可能合并；定时器精度由平台调度决定；
- 渲染像素、系统字体、剪贴板和文件对话框外观不保证跨平台完全一致；
- 不提供同进程原生扩展隔离，也不承诺跨设备统一 FPS 或内存数字。

完整平台和版本矩阵见 [COMPATIBILITY.md](COMPATIBILITY.md)。

## 文档

- [使用指南](docs/GUIDE.md)
- [公开 API](docs/API.md) 与 [机器 API](api/api-v1.json)
- [错误参考](docs/ERRORS.md)
- [架构](docs/ARCHITECTURE.md)
- [安全模型](docs/SECURITY_MODEL.md)
- [性能与容量](docs/PERFORMANCE.md)
- [0.1 到 1.0 迁移](docs/MIGRATION.md)
- [1.0.0 发布说明](docs/RELEASE_NOTES_1.0.0.md)
- [第三方依赖](docs/THIRD_PARTY.md)
- [贡献指南](CONTRIBUTING.md) 与 [安全政策](SECURITY.md)
- [变更日志](CHANGELOG.md)

机器可读 API 和 Markdown API 都由`src/主.yx`生成。参与开发前阅读贡献指南；当前平台
release 构建、公共消费规格和真实窗口冒烟都属于提交门禁。

言窗采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可证。
