# 错误契约

## 读取稳定错误码

言序 1.1.12 把 ABI v2 失败的运行时源代码记为 `NATIVE_V2`，后端稳定码位于受保护的
消息前缀。调用方应使用言窗公开的`错误详情`，不要解析消息：

```yanxu
试 则
    定 窗口 为 应用.窗口（{「宽」：0，「高」：480}）；
救 所误 则
    定 详情 为 界面.错误详情（所误）；
    若 （详情【「代码」】 等于 「GUI_SIZE_RANGE」）则
        言「窗口尺寸无效」；
    终
终
```

返回典包含`代码`、`消息`、`源代码`、`类别`、`位置`和`踪迹`。只有受限的
`[GUI_*]` ABI 前缀会被提升为稳定代码；其他错误保持原运行时代码和消息。

## 输入与配置

| 代码 | 含义 |
| --- | --- |
| `GUI_ARGUMENT_COUNT` | 公开层与原生操作的参数数量不一致 |
| `GUI_VALUE_TYPE` | 值不是该字段要求的文字、数字、逻辑、列、典、字节或资源 |
| `GUI_VALUE_UTF8` | ABI 文字或典键不是有效 UTF-8 |
| `GUI_VALUE_LIMIT` | 聚合值超过元素或总字节预算 |
| `GUI_SIZE_RANGE` | 尺寸不是 1 至 16,384 的有限数，或与控件最小/最大尺寸冲突 |
| `GUI_WINDOW_SIZE` | 窗口初始宽高与最小/最大宽高互相冲突 |
| `GUI_COLOR` | 颜色不是支持的十六进制文字或 3/4 项数值列 |
| `GUI_IMAGE` | 图片格式、尺寸或解码分配不符合限制 |
| `GUI_PROPERTY` | 资源不支持指定属性名 |
| `GUI_THEME` | 主题不是`系统`、`亮色`或`暗色` |

## 布局、控件、画布与定时器

| 代码 | 含义 |
| --- | --- |
| `GUI_LAYOUT_TYPE` | 未知布局种类 |
| `GUI_LAYOUT_ALIGN` | 水平或垂直对齐名称无效 |
| `GUI_LAYOUT_RANGE` | 列数、间距、内边距等布局值超限 |
| `GUI_LAYOUT_FEATURE` | 请求了 1.0 尚未实现的布局能力，例如非零`伸缩` |
| `GUI_CONTROL_TYPE` | 未知控件，或只对特定控件执行了操作 |
| `GUI_CONTROL_RANGE` | 滑块范围、进度值或选择索引无效 |
| `GUI_CANVAS_COMMAND` | 画布命令种类、坐标或字段无效 |
| `GUI_CANVAS_LIMIT` | 画布命令数或估算内存超过上限 |
| `GUI_TIMER_RANGE` | 间隔不在 10 至 86,400,000 毫秒内 |
| `GUI_TIMER_STATE` | 定时器状态转换无效，例如以`假`取消 |

## 资源、事件与回调

| 代码 | 含义 |
| --- | --- |
| `GUI_RESOURCE_CLOSED` | 句柄已关闭，或父资源关闭后节点不再存在 |
| `GUI_RESOURCE_TYPE` | 操作要求的资源种类与句柄不符 |
| `GUI_RESOURCE_LOOP` | 句柄来自另一个宿主事件循环 |
| `GUI_RESOURCE_THREAD` | 在非创建线程访问资源 |
| `GUI_RESOURCE_LIMIT` | 模型节点数或树深度超过上限 |
| `GUI_EVENT_NAME` | 事件名为空或超过 256 个 UTF-8 字节 |
| `GUI_EVENT_LIMIT` | 单节点不同事件绑定超过 128 个 |
| `GUI_CALLBACK_RELEASED` | 回调在后端保留前已经失效 |
| `GUI_CALLBACK_POST` | 宿主拒绝事件投递 |
| `GUI_CALLBACK_PUMP` | 宿主无法泵送已投递回调 |

## 权限、平台与宿主

| 代码 | 含义 |
| --- | --- |
| `GUI_PERMISSION` | 运行事件循环时缺少`图形界面`权限 |
| `GUI_PERMISSION_CLIPBOARD` | 缺少`剪贴板`权限 |
| `GUI_PERMISSION_DIALOG` | 缺少`文件对话框`权限 |
| `GUI_CLIPBOARD` | 平台剪贴板初始化或写入失败 |
| `GUI_DIALOG_TYPE` | 文件对话框操作种类无效 |
| `GUI_DIALOG_FILTER` | 文件过滤器结构或扩展名无效 |
| `GUI_HOST_ABI` | 宿主 ABI 版本或结构大小不兼容 |
| `GUI_HOST_MISSING` | 宿主没有提供所需的资源或回调函数 |
| `GUI_FUNCTION` | ABI 调用了未登记的操作编号 |
| `GUI_BACKEND` | eframe 平台事件循环返回错误 |
| `GUI_BACKEND_STATE` | 后端模型锁已中毒；普通操作被隔离 |
| `GUI_BACKEND_PANIC` | Rust panic 被 ABI 边界捕获 |

## 兼容规则

- 1.x 不删除稳定代码，也不改变现有代码的主要含义；
- 补丁或次版本可以增加更具体的新代码；
- 消息是诊断文字，可随平台或版本改变；
- 关闭父资源后，任何子句柄操作都按`GUI_RESOURCE_CLOSED`处理；
- 清理函数内部幂等，但对已关闭句柄再次发起公开操作仍会报告关闭错误；
- `尺寸`、`最小尺寸`和`最大尺寸`各包含两个顺序调用，第二项失败不会回滚已成功的
  第一项；需要原子业务语义时，调用前先校验两项。

未知错误应记录完整`错误详情`并安全终止当前 GUI 操作，不要按消息猜测成功。
