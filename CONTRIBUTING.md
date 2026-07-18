# 参与言窗开发

言窗由言序公开层、Rust ABI v2 后端、平台制品清单和可执行消费项目组成。修改任何一
层时，都应保留这四层之间的契约，不要仅更新其中一处。

## 开发环境

- 言序 1.1.12；
- Rust 1.92，包含 `rustfmt` 与 `clippy`；
- 言包 0.5.0；
- Linux 需要 GTK 3、Wayland、XKB 和 X11 开发库。

以下命令中的 `<仓库>` 是 `yanxu-gui` 的绝对或相对路径。命令可以从仓库外执行，
不依赖当前目录：

```sh
cargo --config <仓库>/.cargo/config.toml fmt \
  --manifest-path <仓库>/Cargo.toml --all -- --check
cargo --config <仓库>/.cargo/config.toml test \
  --manifest-path <仓库>/Cargo.toml --workspace --locked
cargo --config <仓库>/.cargo/config.toml clippy --manifest-path <仓库>/Cargo.toml \
  --workspace --all-targets --all-features --locked -- -D warnings
cargo --config <仓库>/.cargo/config.toml build \
  --manifest-path <仓库>/Cargo.toml --workspace --release --locked
<仓库>/scripts/prepare-current.sh <仓库>
```

`prepare-current.sh` 只生成当前平台的开发制品和被忽略的根 `言序.toml`。不要提交
`dist/`、`target/` 或单平台根清单。

## 言序层验证

固定使用 1.1.12 工具链。先更新并离线验证消费锁，再检查全部示例和规格：

```sh
YANXU_BIN=<yanxu-1.1.12> yanbao 更 --manifest-path <仓库>/examples
YANXU_BIN=<yanxu-1.1.12> yanbao 更 --manifest-path <仓库>/tests
<yanxu-1.1.12> 包 锁 --离线 <仓库>/examples
<yanxu-1.1.12> 包 锁 --离线 <仓库>/tests
for file in <仓库>/examples/*.yx; do <yanxu-1.1.12> 查 "$file"; done
for file in <仓库>/tests/*.yx; do
  <yanxu-1.1.12> 查 "$file"
  <yanxu-1.1.12> 字节 "$file"
done
```

GUI 规格使用字节码入口，因为通用规格沙箱不装载原生模块。真实窗口冒烟另行执行：

```sh
<yanxu-1.1.12> 字节 <仓库>/examples/自动关闭冒烟.yx
```

## API 与文档

修改 `src/主.yx` 的公开声明后，必须重新生成并审查两份制品：

```sh
<yanxu-1.1.12> 文 --json <仓库>/src/主.yx <仓库>/api/api-v1.json
<yanxu-1.1.12> 文 <仓库>/src/主.yx <仓库>/docs/API.md
```

生成结果必须与实现同一提交。新增错误码时更新 `docs/ERRORS.md`；改变兼容边界时更新
`COMPATIBILITY.md` 和 `docs/MIGRATION.md`；新增依赖时更新 `docs/THIRD_PARTY.md`。

## 改动要求

- 每个提交只包含一个可验证目的；
- 原生边界必须返回稳定 `GUI_*` 错误码，不依赖错误消息作程序判断；
- 回调、资源、图片、画布和聚合值必须继续受限；
- 不提交密钥、令牌、签名证书、用户路径或生成的动态库；
- 不绕过格式、测试、Clippy、消费锁、API 漂移和真实窗口门禁。
