# 言窗（yanxu-gui）

言窗是言序 1.1.12 的官方桌面 GUI 包。公开层使用中文言序 API，原生层通过
ABI v2 封装 `eframe/egui + winit`，支持 Windows、macOS、Linux Wayland 与
Linux X11。公开回调使用类型化`事件`对象，不把原生框架类型泄露给应用。

快速创建、运行和构建无需编写 Rust：

```sh
yanbao new 我的窗口 --gui
yanbao run --manifest-path 我的窗口
yanbao build --manifest-path 我的窗口 --release --bundle
```

使用说明见 [docs/GUIDE.md](docs/GUIDE.md)，完整公开签名见
[docs/API.md](docs/API.md)，机器可读 API 为 [api/api-v1.json](api/api-v1.json)，
依赖用途和许可见 [docs/THIRD_PARTY.md](docs/THIRD_PARTY.md)。

从多仓工作区根目录构建当前平台后端并生成带摘要的开发清单：

```sh
cargo build --manifest-path yanxu-libraries-workspace/repos/yanxu-gui/Cargo.toml --release
./yanxu-libraries-workspace/repos/yanxu-gui/scripts/prepare-current.sh \
  yanxu-libraries-workspace/repos/yanxu-gui
```

`prepare-current.sh` 只登记本次实际构建的平台制品；发布包由六目标 CI 分别构建、
校验后合并，运行时不会下载后端。

CI 在 Linux、macOS、Windows 的 x86-64 与 ARM64 原生托管机上分别编译、测试并
核对 `yanxu_native_module_v2` 导出；仓库不把一个平台的动态库冒充其他目标制品。
