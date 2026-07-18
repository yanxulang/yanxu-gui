use crate::abi::{self, NativeError, NativeHost};
use crate::bridge::{encode_data, free_value};
use crate::model::*;
use eframe::egui;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TYPE_APP: &[u8] = b"yanxu.gui.application";
const TYPE_WINDOW: &[u8] = b"yanxu.gui.window";
const TYPE_LAYOUT: &[u8] = b"yanxu.gui.layout";
const TYPE_CONTROL: &[u8] = b"yanxu.gui.control";
const TYPE_TIMER: &[u8] = b"yanxu.gui.timer";

#[derive(Clone, Copy)]
pub struct HostApi(pub NativeHost);

impl HostApi {
    fn retain(self, callback: u64) -> Result<(), &'static str> {
        let function = self.0.callback_retain.ok_or("GUI_HOST_MISSING")?;
        (unsafe { function(self.0.context, callback) } == abi::OK)
            .then_some(())
            .ok_or("GUI_CALLBACK_RELEASED")
    }

    fn release(self, callback: u64) {
        if let Some(function) = self.0.callback_release {
            let _ = unsafe { function(self.0.context, callback) };
        }
    }

    fn permission(self, name: &str) -> bool {
        self.0.has_permission.is_some_and(|function| unsafe {
            function(self.0.context, name.as_ptr(), name.len()) != 0
        })
    }

    fn post(self, callback: u64, event: Data) -> Result<(), &'static str> {
        let function = self.0.callback_post.ok_or("GUI_HOST_MISSING")?;
        let mut value = encode_data(event);
        let mut error = NativeError::default();
        let result = unsafe { function(self.0.context, callback, &value, 1, &mut error) };
        unsafe { free_value(&mut value) };
        (result == abi::OK).then_some(()).ok_or("GUI_CALLBACK_POST")
    }

    fn pump(self) -> Result<(), &'static str> {
        let Some(function) = self.0.pump else {
            return Ok(());
        };
        let mut error = NativeError::default();
        (unsafe { function(self.0.context, 4096, &mut error) } == abi::OK)
            .then_some(())
            .ok_or("GUI_CALLBACK_PUMP")
    }
}

pub struct GuiResource {
    pub model: Arc<Mutex<Model>>,
    pub kind: ResourceKind,
    pub id: u64,
    pub host: HostApi,
    cleaned: AtomicBool,
}

impl GuiResource {
    fn cleanup(&self) {
        if self.cleaned.swap(true, Ordering::AcqRel) {
            return;
        }
        let callbacks = lock_model_for_cleanup(&self.model).remove(self.id);
        for callback in callbacks {
            self.host.release(callback);
        }
    }
}

impl Drop for GuiResource {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub unsafe extern "C" fn drop_gui_resource(resource: *mut c_void) {
    if !resource.is_null() {
        drop(unsafe { Box::from_raw(resource.cast::<GuiResource>()) });
    }
}

pub struct ResourceOutput {
    pub resource: Box<GuiResource>,
    pub type_name: &'static [u8],
    pub parent: u64,
}

pub enum Output {
    Value(Data),
    Resource(ResourceOutput),
}

fn lock_model(model: &Mutex<Model>) -> Result<MutexGuard<'_, Model>, &'static str> {
    model.lock().map_err(|_| "GUI_BACKEND_STATE")
}

fn lock_model_for_cleanup(model: &Mutex<Model>) -> MutexGuard<'_, Model> {
    match model.lock() {
        Ok(model) => model,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum Operation {
    CreateApplication = 1,
    CreateWindow,
    CreateLayout,
    CreateControl,
    SetProperty,
    GetProperty,
    BindEvent,
    Show,
    Hide,
    Close,
    Run,
    Exit,
    CreateTimer,
    SetTheme,
    ClipboardRead,
    ClipboardWrite,
    Dialog,
    CanvasCommand,
    EmitCustom,
    DebugSnapshot,
}

impl Operation {
    pub fn from_context(context: *mut c_void) -> Option<Self> {
        Some(match context as usize {
            1 => Self::CreateApplication,
            2 => Self::CreateWindow,
            3 => Self::CreateLayout,
            4 => Self::CreateControl,
            5 => Self::SetProperty,
            6 => Self::GetProperty,
            7 => Self::BindEvent,
            8 => Self::Show,
            9 => Self::Hide,
            10 => Self::Close,
            11 => Self::Run,
            12 => Self::Exit,
            13 => Self::CreateTimer,
            14 => Self::SetTheme,
            15 => Self::ClipboardRead,
            16 => Self::ClipboardWrite,
            17 => Self::Dialog,
            18 => Self::CanvasCommand,
            19 => Self::EmitCustom,
            20 => Self::DebugSnapshot,
            _ => return None,
        })
    }
}

pub unsafe fn call(
    operation: Operation,
    arguments: &[Data],
    host: HostApi,
) -> Result<Output, &'static str> {
    if host.0.abi_version != abi::ABI || host.0.struct_size < std::mem::size_of::<NativeHost>() {
        return Err("GUI_HOST_ABI");
    }
    match operation {
        Operation::CreateApplication => {
            require_count(arguments, 1)?;
            let title = text(&arguments[0])?.to_owned();
            let model = Arc::new(Mutex::new(Model::default()));
            let id = lock_model(&model)?.create(
                None,
                NodeKind::Application {
                    title,
                    theme: "系统".into(),
                },
            )?;
            Ok(resource_output(
                model,
                ResourceKind::Application,
                id,
                host,
                TYPE_APP,
                0,
            ))
        }
        Operation::CreateWindow => {
            require_count(arguments, 2)?;
            let (parent_handle, parent) =
                unsafe { resource(arguments, host, ResourceKind::Application) }?;
            let config = map(&arguments[1])?;
            let mut window = WindowState::default();
            apply_window_config(&mut window, config)?;
            let id =
                lock_model(&parent.model)?.create(Some(parent.id), NodeKind::Window(window))?;
            Ok(resource_output(
                parent.model.clone(),
                ResourceKind::Window,
                id,
                host,
                TYPE_WINDOW,
                parent_handle,
            ))
        }
        Operation::CreateLayout => {
            require_count(arguments, 3)?;
            let (parent_handle, parent) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[ResourceKind::Window, ResourceKind::Layout],
                )
            }?;
            let kind = layout_kind(text(&arguments[1])?)?;
            let config = map(&arguments[2])?;
            let mut layout = LayoutState::new(kind);
            apply_layout_config(&mut layout, config)?;
            let id =
                lock_model(&parent.model)?.create(Some(parent.id), NodeKind::Layout(layout))?;
            Ok(resource_output(
                parent.model.clone(),
                ResourceKind::Layout,
                id,
                host,
                TYPE_LAYOUT,
                parent_handle,
            ))
        }
        Operation::CreateControl => {
            require_count(arguments, 3)?;
            let (parent_handle, parent) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[ResourceKind::Window, ResourceKind::Layout],
                )
            }?;
            let config = map(&arguments[2])?;
            let mut control = create_control(text(&arguments[1])?, config)?;
            apply_control_config(&mut control, config)?;
            let id =
                lock_model(&parent.model)?.create(Some(parent.id), NodeKind::Control(control))?;
            Ok(resource_output(
                parent.model.clone(),
                ResourceKind::Control,
                id,
                host,
                TYPE_CONTROL,
                parent_handle,
            ))
        }
        Operation::SetProperty => {
            require_count(arguments, 3)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Application,
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                        ResourceKind::Timer,
                    ],
                )
            }?;
            if let Some(callback) = set_property(resource, text(&arguments[1])?, &arguments[2])? {
                host.release(callback);
            }
            Ok(Output::Value(Data::Nil))
        }
        Operation::GetProperty => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Application,
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                        ResourceKind::Timer,
                    ],
                )
            }?;
            Ok(Output::Value(get_property(resource, text(&arguments[1])?)?))
        }
        Operation::BindEvent => {
            require_count(arguments, 3)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Application,
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                    ],
                )
            }?;
            let event = event_name(&arguments[1])?.to_owned();
            let callback = callback(&arguments[2])?;
            host.retain(callback)?;
            let result = lock_model(&resource.model)
                .and_then(|mut model| model.bind_event(resource.id, event, callback));
            match result {
                Ok(previous) => {
                    if let Some(previous) = previous {
                        host.release(previous);
                    }
                    Ok(Output::Value(Data::Nil))
                }
                Err(error) => {
                    host.release(callback);
                    Err(error)
                }
            }
        }
        Operation::Show | Operation::Hide => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                    ],
                )
            }?;
            lock_model(&resource.model)?
                .node_mut(resource.id)?
                .common
                .visible = matches!(operation, Operation::Show);
            Ok(Output::Value(Data::Nil))
        }
        Operation::Close => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Application,
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                        ResourceKind::Timer,
                    ],
                )
            }?;
            resource.cleanup();
            Ok(Output::Value(Data::Nil))
        }
        Operation::Run => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource(arguments, host, ResourceKind::Application) }?;
            run(resource.model.clone(), host)?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::Exit => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource(arguments, host, ResourceKind::Application) }?;
            let mut model = lock_model(&resource.model)?;
            model.exit_requested = true;
            model.repaint_requested = true;
            if let Some(wake) = host.0.wake {
                unsafe { wake(host.0.context) };
            }
            Ok(Output::Value(Data::Nil))
        }
        Operation::CreateTimer => {
            require_count(arguments, 4)?;
            let (parent_handle, parent) =
                unsafe { resource(arguments, host, ResourceKind::Application) }?;
            let milliseconds = arguments[1]
                .as_u32()
                .filter(|value| (10..=86_400_000).contains(value))
                .ok_or("GUI_TIMER_RANGE")?;
            let repeating = arguments[2].as_bool().ok_or("GUI_VALUE_TYPE")?;
            let callback = callback(&arguments[3])?;
            host.retain(callback)?;
            let timer = TimerState {
                interval: Duration::from_millis(u64::from(milliseconds)),
                repeating,
                next: Instant::now() + Duration::from_millis(u64::from(milliseconds)),
                callback: Some(callback),
                cancelled: false,
            };
            let result = lock_model(&parent.model)
                .and_then(|mut model| model.create(Some(parent.id), NodeKind::Timer(timer)));
            let id = match result {
                Ok(id) => id,
                Err(error) => {
                    host.release(callback);
                    return Err(error);
                }
            };
            Ok(resource_output(
                parent.model.clone(),
                ResourceKind::Timer,
                id,
                host,
                TYPE_TIMER,
                parent_handle,
            ))
        }
        Operation::SetTheme => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe { resource(arguments, host, ResourceKind::Application) }?;
            let theme = text(&arguments[1])?;
            if !matches!(theme, "亮色" | "暗色" | "系统") {
                return Err("GUI_THEME");
            }
            let mut model = lock_model(&resource.model)?;
            let NodeKind::Application { theme: current, .. } =
                &mut model.node_mut(resource.id)?.kind
            else {
                return Err("GUI_RESOURCE_TYPE");
            };
            *current = theme.into();
            Ok(Output::Value(Data::Nil))
        }
        Operation::ClipboardRead => {
            require_count(arguments, 0)?;
            if !host.permission("剪贴板") {
                return Err("GUI_PERMISSION_CLIPBOARD");
            }
            let mut clipboard = arboard::Clipboard::new().map_err(|_| "GUI_CLIPBOARD")?;
            match clipboard.get_text() {
                Ok(text) => Ok(Output::Value(Data::String(text))),
                Err(_) => Ok(Output::Value(Data::Nil)),
            }
        }
        Operation::ClipboardWrite => {
            require_count(arguments, 1)?;
            if !host.permission("剪贴板") {
                return Err("GUI_PERMISSION_CLIPBOARD");
            }
            let contents = text(&arguments[0])?.to_owned();
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set_text(contents))
                .map_err(|_| "GUI_CLIPBOARD")?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::Dialog => {
            require_count(arguments, 2)?;
            if !host.permission("文件对话框") {
                return Err("GUI_PERMISSION_DIALOG");
            }
            Ok(Output::Value(dialog(
                text(&arguments[0])?,
                map(&arguments[1])?,
            )?))
        }
        Operation::CanvasCommand => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe { resource(arguments, host, ResourceKind::Control) }?;
            let command = canvas_command(map(&arguments[1])?)?;
            let mut model = lock_model(&resource.model)?;
            let NodeKind::Control(control) = &mut model.node_mut(resource.id)?.kind else {
                return Err("GUI_RESOURCE_TYPE");
            };
            let ControlKind::Canvas { commands } = &mut control.kind else {
                return Err("GUI_CONTROL_TYPE");
            };
            if matches!(command, CanvasCommand::Clear(_)) {
                commands.clear();
            }
            if commands.len() >= 65_536 {
                return Err("GUI_CANVAS_LIMIT");
            }
            commands.push(command);
            Ok(Output::Value(Data::Nil))
        }
        Operation::EmitCustom => {
            require_count(arguments, 3)?;
            let (_, resource) = unsafe {
                resource_any(
                    arguments,
                    host,
                    &[
                        ResourceKind::Application,
                        ResourceKind::Window,
                        ResourceKind::Layout,
                        ResourceKind::Control,
                    ],
                )
            }?;
            let event = event_name(&arguments[1])?;
            let node = lock_model(&resource.model)?.node(resource.id)?.clone();
            if let Some(callback) = node.events.get(event) {
                host.post(
                    *callback,
                    custom_event_data(event, &node, arguments[2].clone()),
                )?;
                host.pump()?;
            }
            Ok(Output::Value(Data::Nil))
        }
        Operation::DebugSnapshot => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource(arguments, host, ResourceKind::Application) }?;
            let model = lock_model(&resource.model)?;
            let mut result = BTreeMap::new();
            result.insert("资源总数".into(), Data::Integer(model.nodes.len() as i64));
            for (kind, count) in model.counts() {
                result.insert(kind, Data::Integer(count as i64));
            }
            Ok(Output::Value(Data::Map(result)))
        }
    }
}

fn resource_output(
    model: Arc<Mutex<Model>>,
    kind: ResourceKind,
    id: u64,
    host: HostApi,
    type_name: &'static [u8],
    parent: u64,
) -> Output {
    Output::Resource(ResourceOutput {
        resource: Box::new(GuiResource {
            model,
            kind,
            id,
            host,
            cleaned: AtomicBool::new(false),
        }),
        type_name,
        parent,
    })
}

unsafe fn resource<'a>(
    arguments: &[Data],
    host: HostApi,
    kind: ResourceKind,
) -> Result<(u64, &'a GuiResource), &'static str> {
    unsafe { resource_any(arguments, host, &[kind]) }
}

unsafe fn resource_any<'a>(
    arguments: &[Data],
    host: HostApi,
    kinds: &[ResourceKind],
) -> Result<(u64, &'a GuiResource), &'static str> {
    let Data::Resource(handle) = arguments.first().ok_or("GUI_ARGUMENT_COUNT")? else {
        return Err("GUI_VALUE_TYPE");
    };
    let getter = host.0.resource_get.ok_or("GUI_HOST_MISSING")?;
    let mut raw = std::ptr::null_mut();
    if unsafe { getter(host.0.context, *handle, &mut raw) } != abi::OK || raw.is_null() {
        return Err("GUI_RESOURCE_CLOSED");
    }
    let resource = unsafe { &*raw.cast::<GuiResource>() };
    if !kinds.contains(&resource.kind) || resource.cleaned.load(Ordering::Acquire) {
        return Err("GUI_RESOURCE_TYPE");
    }
    if resource.host.0.event_loop_id != host.0.event_loop_id {
        return Err("GUI_RESOURCE_LOOP");
    }
    lock_model(&resource.model)?
        .node_mut(resource.id)?
        .public_handle = *handle;
    Ok((*handle, resource))
}

fn require_count(arguments: &[Data], expected: usize) -> Result<(), &'static str> {
    (arguments.len() == expected)
        .then_some(())
        .ok_or("GUI_ARGUMENT_COUNT")
}

fn text(value: &Data) -> Result<&str, &'static str> {
    value.as_text().ok_or("GUI_VALUE_TYPE")
}

fn event_name(value: &Data) -> Result<&str, &'static str> {
    let event = text(value)?;
    (!event.is_empty() && event.len() <= 256)
        .then_some(event)
        .ok_or("GUI_EVENT_NAME")
}

fn map(value: &Data) -> Result<&BTreeMap<String, Data>, &'static str> {
    match value {
        Data::Map(value) => Ok(value),
        _ => Err("GUI_VALUE_TYPE"),
    }
}

fn callback(value: &Data) -> Result<u64, &'static str> {
    match value {
        Data::Callback(value) => Ok(*value),
        _ => Err("GUI_VALUE_TYPE"),
    }
}

fn map_text(map: &BTreeMap<String, Data>, key: &str) -> Option<String> {
    map.get(key).and_then(Data::as_text).map(str::to_owned)
}

fn map_bool(map: &BTreeMap<String, Data>, key: &str) -> Option<bool> {
    map.get(key).and_then(Data::as_bool)
}

fn map_number(map: &BTreeMap<String, Data>, key: &str) -> Option<f64> {
    map.get(key).and_then(Data::as_f64)
}

fn map_strings(map: &BTreeMap<String, Data>, key: &str) -> Option<Vec<String>> {
    match map.get(key)? {
        Data::Array(values) => values
            .iter()
            .map(|value| value.as_text().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

fn apply_window_config(
    window: &mut WindowState,
    config: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    window.title = map_text(config, "标题").unwrap_or_else(|| window.title.clone());
    window.width = positive_f32(map_number(config, "宽"), window.width)?;
    window.height = positive_f32(map_number(config, "高"), window.height)?;
    window.minimum_width = positive_f32(map_number(config, "最小宽"), window.minimum_width)?;
    window.minimum_height = positive_f32(map_number(config, "最小高"), window.minimum_height)?;
    window.maximum_width = optional_positive_f32(config.get("最大宽"))?;
    window.maximum_height = optional_positive_f32(config.get("最大高"))?;
    if window.minimum_width > window.width
        || window.minimum_height > window.height
        || window
            .maximum_width
            .is_some_and(|value| value < window.width)
        || window
            .maximum_height
            .is_some_and(|value| value < window.height)
    {
        return Err("GUI_WINDOW_SIZE");
    }
    window.resizable = map_bool(config, "可缩放").unwrap_or(true);
    window.high_dpi = map_bool(config, "高分屏").unwrap_or(true);
    window.always_on_top = map_bool(config, "置顶").unwrap_or(false);
    window.centered = map_bool(config, "居中").unwrap_or(false);
    if let Some(Data::Bytes(icon)) = config.get("图标") {
        image::load_from_memory(icon).map_err(|_| "GUI_IMAGE")?;
        window.icon = Some(icon.clone());
    }
    Ok(())
}

fn positive_f32(value: Option<f64>, default: f32) -> Result<f32, &'static str> {
    match value {
        None => Ok(default),
        Some(value) if value.is_finite() && (1.0..=16_384.0).contains(&value) => Ok(value as f32),
        _ => Err("GUI_SIZE_RANGE"),
    }
}

fn optional_positive_f32(value: Option<&Data>) -> Result<Option<f32>, &'static str> {
    match value {
        None | Some(Data::Nil) => Ok(None),
        Some(value) => positive_f32(value.as_f64(), 0.0).map(Some),
    }
}

fn layout_kind(kind: &str) -> Result<LayoutKind, &'static str> {
    match kind {
        "纵向" => Ok(LayoutKind::Vertical),
        "横向" => Ok(LayoutKind::Horizontal),
        "网格" => Ok(LayoutKind::Grid),
        "层叠" => Ok(LayoutKind::Stack),
        "滚动" => Ok(LayoutKind::Scroll),
        _ => Err("GUI_LAYOUT_TYPE"),
    }
}

fn apply_layout_config(
    layout: &mut LayoutState,
    config: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    if let Some(columns) = config.get("列数").and_then(Data::as_u32) {
        if !(1..=64).contains(&columns) {
            return Err("GUI_LAYOUT_RANGE");
        }
        layout.columns = columns as usize;
    }
    layout.spacing = map_number(config, "间距").unwrap_or(8.0).clamp(0.0, 512.0) as f32;
    layout.padding = map_number(config, "内边距")
        .unwrap_or(8.0)
        .clamp(0.0, 512.0) as f32;
    layout.grow = map_number(config, "伸缩").unwrap_or(0.0).clamp(0.0, 1000.0) as f32;
    layout.horizontal_alignment = map_text(config, "水平对齐").unwrap_or_else(|| "左".into());
    layout.vertical_alignment = map_text(config, "垂直对齐").unwrap_or_else(|| "上".into());
    Ok(())
}

fn create_control(
    kind: &str,
    config: &BTreeMap<String, Data>,
) -> Result<ControlState, &'static str> {
    let kind = match kind {
        "文字" => ControlKind::Text,
        "按钮" => ControlKind::Button,
        "输入框" => ControlKind::Input {
            multiline: false,
            placeholder: map_text(config, "占位").unwrap_or_default(),
        },
        "多行输入框" => ControlKind::Input {
            multiline: true,
            placeholder: map_text(config, "占位").unwrap_or_default(),
        },
        "复选框" => ControlKind::Checkbox,
        "单选框" => ControlKind::Radio {
            group: map_text(config, "组").unwrap_or_default(),
        },
        "下拉选择" => ControlKind::Select {
            options: map_strings(config, "选项").unwrap_or_default(),
        },
        "滑块" => ControlKind::Slider {
            minimum: map_number(config, "最小值").unwrap_or(0.0),
            maximum: map_number(config, "最大值").unwrap_or(100.0),
        },
        "进度条" => ControlKind::Progress,
        "图片" => ControlKind::Image {
            bytes: match config.get("图片") {
                Some(Data::Bytes(bytes)) => bytes.clone(),
                _ => Vec::new(),
            },
            preserve_ratio: map_bool(config, "保持比例").unwrap_or(true),
        },
        "列表" => ControlKind::List {
            items: map_strings(config, "项目").unwrap_or_default(),
        },
        "分隔线" => ControlKind::Separator,
        "标签页" => ControlKind::Tabs {
            tabs: map_strings(config, "标签").unwrap_or_default(),
        },
        "菜单" => ControlKind::Menu {
            items: map_strings(config, "项目").unwrap_or_default(),
        },
        "Canvas" | "画布" => ControlKind::Canvas {
            commands: Vec::new(),
        },
        _ => return Err("GUI_CONTROL_TYPE"),
    };
    Ok(ControlState::new(kind))
}

fn apply_control_config(
    control: &mut ControlState,
    config: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    control.text = map_text(config, "文字")
        .or_else(|| map_text(config, "内容"))
        .unwrap_or_default();
    control.selected = map_bool(config, "选中").unwrap_or(false);
    control.value = map_number(config, "值").unwrap_or(0.0);
    control.selected_index = config.get("当前项").and_then(Data::as_u32).unwrap_or(0) as usize;
    control.text_color = config.get("文字颜色").map(color).transpose()?;
    control.background_color = config.get("背景颜色").map(color).transpose()?;
    control.border_color = config.get("边框颜色").map(color).transpose()?;
    control.border_width = map_number(config, "边框宽度")
        .unwrap_or(0.0)
        .clamp(0.0, 64.0) as f32;
    control.corner_radius = map_number(config, "圆角").unwrap_or(4.0).clamp(0.0, 512.0) as f32;
    control.font_family = map_text(config, "字体").unwrap_or_default();
    control.font_size = map_number(config, "字号").unwrap_or(14.0).clamp(6.0, 256.0) as f32;
    control.font_weight = map_number(config, "字重")
        .unwrap_or(400.0)
        .clamp(100.0, 900.0) as u16;
    control.padding = map_number(config, "内边距")
        .unwrap_or(4.0)
        .clamp(0.0, 512.0) as f32;
    control.margin = map_number(config, "外边距")
        .unwrap_or(0.0)
        .clamp(0.0, 512.0) as f32;
    Ok(())
}

fn set_property(
    resource: &GuiResource,
    key: &str,
    value: &Data,
) -> Result<Option<u64>, &'static str> {
    let mut model = lock_model(&resource.model)?;
    let node = model.node_mut(resource.id)?;
    let mut callback_to_release = None;
    match key {
        "可见" => node.common.visible = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
        "启用" => node.common.enabled = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
        "宽" => node.common.width = optional_positive_f32(Some(value))?,
        "高" => node.common.height = optional_positive_f32(Some(value))?,
        "最小宽" => node.common.minimum_width = optional_positive_f32(Some(value))?,
        "最小高" => node.common.minimum_height = optional_positive_f32(Some(value))?,
        "最大宽" => node.common.maximum_width = optional_positive_f32(Some(value))?,
        "最大高" => node.common.maximum_height = optional_positive_f32(Some(value))?,
        "工具提示" => node.common.tooltip = text(value)?.into(),
        "焦点" => node.common.focus_requested = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
        "样式类" => node.common.style_class = text(value)?.into(),
        "可访问名称" => node.common.accessible_name = text(value)?.into(),
        "可访问描述" => node.common.accessible_description = text(value)?.into(),
        _ => match &mut node.kind {
            NodeKind::Application { title, theme } => match key {
                "名称" => *title = text(value)?.into(),
                "主题" => *theme = text(value)?.into(),
                _ => return Err("GUI_PROPERTY"),
            },
            NodeKind::Window(window) => match key {
                "标题" => window.title = text(value)?.into(),
                "最大化" => window.maximized = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "最小化" => window.minimized = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "全屏" => window.fullscreen = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "置顶" => window.always_on_top = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "可缩放" => window.resizable = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "居中" => window.centered = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
                "图标" => {
                    let Data::Bytes(bytes) = value else {
                        return Err("GUI_VALUE_TYPE");
                    };
                    image::load_from_memory(bytes).map_err(|_| "GUI_IMAGE")?;
                    window.icon = Some(bytes.clone());
                }
                _ => return Err("GUI_PROPERTY"),
            },
            NodeKind::Layout(layout) => match key {
                "间距" => {
                    layout.spacing =
                        value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 512.0) as f32
                }
                "内边距" => {
                    layout.padding =
                        value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 512.0) as f32
                }
                "伸缩" => {
                    layout.grow = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 1000.0) as f32
                }
                _ => return Err("GUI_PROPERTY"),
            },
            NodeKind::Control(control) => set_control_property(control, key, value)?,
            NodeKind::Timer(timer) => match key {
                "取消" => {
                    if !value.as_bool().ok_or("GUI_VALUE_TYPE")? {
                        return Err("GUI_TIMER_STATE");
                    }
                    timer.cancelled = true;
                    callback_to_release = timer.callback.take();
                }
                _ => return Err("GUI_PROPERTY"),
            },
        },
    }
    Ok(callback_to_release)
}

fn set_control_property(
    control: &mut ControlState,
    key: &str,
    value: &Data,
) -> Result<(), &'static str> {
    match key {
        "文字" | "内容" => control.text = text(value)?.into(),
        "选中" => control.selected = value.as_bool().ok_or("GUI_VALUE_TYPE")?,
        "值" => control.value = value.as_f64().ok_or("GUI_VALUE_TYPE")?,
        "当前项" => control.selected_index = value.as_u32().ok_or("GUI_VALUE_TYPE")? as usize,
        "图片" => {
            let Data::Bytes(bytes) = value else {
                return Err("GUI_VALUE_TYPE");
            };
            image::load_from_memory(bytes).map_err(|_| "GUI_IMAGE")?;
            let ControlKind::Image { bytes: current, .. } = &mut control.kind else {
                return Err("GUI_CONTROL_TYPE");
            };
            *current = bytes.clone();
        }
        "文字颜色" => control.text_color = Some(color(value)?),
        "背景颜色" => control.background_color = Some(color(value)?),
        "边框颜色" => control.border_color = Some(color(value)?),
        "边框宽度" => {
            control.border_width = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 64.0) as f32
        }
        "圆角" => {
            control.corner_radius = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 512.0) as f32
        }
        "字体" => control.font_family = text(value)?.into(),
        "字号" => {
            control.font_size = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(6.0, 256.0) as f32
        }
        "字重" => {
            control.font_weight = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(100.0, 900.0) as u16
        }
        "内边距" => {
            control.padding = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 512.0) as f32
        }
        "外边距" => {
            control.margin = value.as_f64().ok_or("GUI_VALUE_TYPE")?.clamp(0.0, 512.0) as f32
        }
        _ => return Err("GUI_PROPERTY"),
    }
    Ok(())
}

fn get_property(resource: &GuiResource, key: &str) -> Result<Data, &'static str> {
    let model = lock_model(&resource.model)?;
    let node = model.node(resource.id)?;
    match key {
        "可见" => return Ok(Data::Bool(node.common.visible)),
        "启用" => return Ok(Data::Bool(node.common.enabled)),
        "宽" => {
            return Ok(node
                .common
                .width
                .map_or(Data::Nil, |value| Data::Number(value.into())));
        }
        "高" => {
            return Ok(node
                .common
                .height
                .map_or(Data::Nil, |value| Data::Number(value.into())));
        }
        "工具提示" => return Ok(Data::String(node.common.tooltip.clone())),
        "样式类" => return Ok(Data::String(node.common.style_class.clone())),
        "可访问名称" => return Ok(Data::String(node.common.accessible_name.clone())),
        "可访问描述" => return Ok(Data::String(node.common.accessible_description.clone())),
        _ => {}
    }
    match &node.kind {
        NodeKind::Application { title, theme } => match key {
            "名称" => Ok(Data::String(title.clone())),
            "主题" => Ok(Data::String(theme.clone())),
            _ => Err("GUI_PROPERTY"),
        },
        NodeKind::Window(window) => match key {
            "标题" => Ok(Data::String(window.title.clone())),
            "DPI" => Ok(Data::Number(window.dpi.into())),
            "位置" => Ok(window.position.map_or(Data::Nil, |position| {
                Data::Array(vec![
                    Data::Number(position[0].into()),
                    Data::Number(position[1].into()),
                ])
            })),
            "最大化" => Ok(Data::Bool(window.maximized)),
            "最小化" => Ok(Data::Bool(window.minimized)),
            "全屏" => Ok(Data::Bool(window.fullscreen)),
            _ => Err("GUI_PROPERTY"),
        },
        NodeKind::Layout(layout) => match key {
            "间距" => Ok(Data::Number(layout.spacing.into())),
            "内边距" => Ok(Data::Number(layout.padding.into())),
            _ => Err("GUI_PROPERTY"),
        },
        NodeKind::Control(control) => match key {
            "文字" | "内容" => Ok(Data::String(control.text.clone())),
            "选中" => Ok(Data::Bool(control.selected)),
            "值" => Ok(Data::Number(control.value)),
            "当前项" => Ok(Data::Integer(control.selected_index as i64)),
            _ => Err("GUI_PROPERTY"),
        },
        NodeKind::Timer(timer) => match key {
            "已取消" => Ok(Data::Bool(timer.cancelled)),
            _ => Err("GUI_PROPERTY"),
        },
    }
}

fn color(value: &Data) -> Result<[u8; 4], &'static str> {
    match value {
        Data::String(value) => parse_hex_color(value),
        Data::Array(values) if matches!(values.len(), 3 | 4) => {
            let mut color = [0_u8, 0, 0, 255];
            for (index, value) in values.iter().enumerate() {
                color[index] = value
                    .as_f64()
                    .filter(|value| (0.0..=255.0).contains(value))
                    .ok_or("GUI_COLOR")? as u8;
            }
            Ok(color)
        }
        _ => Err("GUI_COLOR"),
    }
}

fn parse_hex_color(value: &str) -> Result<[u8; 4], &'static str> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if !matches!(value.len(), 6 | 8) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("GUI_COLOR");
    }
    let mut color = [0, 0, 0, 255];
    for (index, component) in value.as_bytes().chunks_exact(2).enumerate() {
        color[index] = u8::from_str_radix(std::str::from_utf8(component).unwrap(), 16)
            .map_err(|_| "GUI_COLOR")?;
    }
    Ok(color)
}

fn dialog(kind: &str, config: &BTreeMap<String, Data>) -> Result<Data, &'static str> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = map_text(config, "标题") {
        dialog = dialog.set_title(title);
    }
    if let Some(directory) = map_text(config, "目录") {
        dialog = dialog.set_directory(directory);
    }
    if let Some(name) = map_text(config, "文件名") {
        dialog = dialog.set_file_name(name);
    }
    if let Some(Data::Array(filters)) = config.get("过滤") {
        for filter in filters {
            let Data::Map(filter) = filter else {
                return Err("GUI_DIALOG_FILTER");
            };
            let name = map_text(filter, "名称").ok_or("GUI_DIALOG_FILTER")?;
            let extensions = map_strings(filter, "扩展名").ok_or("GUI_DIALOG_FILTER")?;
            let refs = extensions.iter().map(String::as_str).collect::<Vec<_>>();
            dialog = dialog.add_filter(name, &refs);
        }
    }
    Ok(match kind {
        "打开文件" => dialog.pick_file().map_or(Data::Nil, path_data),
        "打开多个文件" => dialog.pick_files().map_or(Data::Nil, |paths| {
            Data::Array(paths.into_iter().map(path_data).collect())
        }),
        "保存文件" => dialog.save_file().map_or(Data::Nil, path_data),
        "选择目录" => dialog.pick_folder().map_or(Data::Nil, path_data),
        _ => return Err("GUI_DIALOG_TYPE"),
    })
}

fn path_data(path: std::path::PathBuf) -> Data {
    Data::String(path.to_string_lossy().into_owned())
}

fn canvas_command(map: &BTreeMap<String, Data>) -> Result<CanvasCommand, &'static str> {
    let kind = map_text(map, "类型").ok_or("GUI_CANVAS_COMMAND")?;
    let point = |name: &str| -> Result<[f32; 2], &'static str> {
        let Data::Array(values) = map.get(name).ok_or("GUI_CANVAS_COMMAND")? else {
            return Err("GUI_CANVAS_COMMAND");
        };
        if values.len() != 2 {
            return Err("GUI_CANVAS_COMMAND");
        }
        Ok([
            values[0].as_f64().ok_or("GUI_CANVAS_COMMAND")? as f32,
            values[1].as_f64().ok_or("GUI_CANVAS_COMMAND")? as f32,
        ])
    };
    let rgba = |name: &str, fallback: [u8; 4]| -> Result<[u8; 4], &'static str> {
        map.get(name)
            .map(color)
            .transpose()
            .map(|value| value.unwrap_or(fallback))
    };
    match kind.as_str() {
        "清空" => Ok(CanvasCommand::Clear(rgba("颜色", [0, 0, 0, 0])?)),
        "线段" => Ok(CanvasCommand::Line {
            from: point("起点")?,
            to: point("终点")?,
            color: rgba("颜色", [255, 255, 255, 255])?,
            width: map_number(map, "宽度").unwrap_or(1.0).clamp(0.1, 256.0) as f32,
        }),
        "矩形" => Ok(CanvasCommand::Rectangle {
            minimum: point("起点")?,
            maximum: point("终点")?,
            color: rgba("颜色", [0, 0, 0, 0])?,
            stroke: rgba("边框颜色", [255, 255, 255, 255])?,
            stroke_width: map_number(map, "边框宽度").unwrap_or(1.0) as f32,
            radius: map_number(map, "圆角").unwrap_or(0.0) as f32,
        }),
        "圆" => Ok(CanvasCommand::Circle {
            center: point("圆心")?,
            radius: map_number(map, "半径").ok_or("GUI_CANVAS_COMMAND")? as f32,
            color: rgba("颜色", [0, 0, 0, 0])?,
            stroke: rgba("边框颜色", [255, 255, 255, 255])?,
            stroke_width: map_number(map, "边框宽度").unwrap_or(1.0) as f32,
        }),
        "文字" => Ok(CanvasCommand::Text {
            position: point("位置")?,
            text: map_text(map, "文字").ok_or("GUI_CANVAS_COMMAND")?,
            color: rgba("颜色", [255, 255, 255, 255])?,
            size: map_number(map, "字号").unwrap_or(14.0) as f32,
        }),
        "图片" => Ok(CanvasCommand::Image {
            minimum: point("起点")?,
            maximum: point("终点")?,
            bytes: match map.get("图片") {
                Some(Data::Bytes(bytes)) => bytes.clone(),
                _ => return Err("GUI_CANVAS_COMMAND"),
            },
        }),
        "裁剪" => Ok(CanvasCommand::Clip {
            minimum: point("起点")?,
            maximum: point("终点")?,
        }),
        "变换" => Ok(CanvasCommand::Transform {
            translation: point("平移")?,
            scale: point("缩放")?,
        }),
        _ => Err("GUI_CANVAS_COMMAND"),
    }
}

fn run(model: Arc<Mutex<Model>>, host: HostApi) -> Result<(), &'static str> {
    if !host.permission("图形界面") {
        return Err("GUI_PERMISSION");
    }
    let (title, viewport) = {
        let model = lock_model(&model)?;
        let (title, window) = model
            .nodes
            .values()
            .find_map(|node| match &node.kind {
                NodeKind::Window(window) => Some((window.title.clone(), window.clone())),
                _ => None,
            })
            .unwrap_or_else(|| ("言窗应用".into(), WindowState::default()));
        let viewport = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([window.width, window.height])
            .with_min_inner_size([window.minimum_width, window.minimum_height])
            .with_resizable(window.resizable)
            .with_visible(true);
        (title, viewport)
    };
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    let run_model = Arc::clone(&model);
    let result = eframe::run_native(
        &title,
        options,
        Box::new(move |_creation_context| {
            Ok(Box::new(DesktopApp {
                model,
                host,
                root_window: None,
                textures: HashMap::new(),
                pending: Vec::new(),
                fonts_loaded: false,
            }))
        }),
    );
    let callbacks = lock_model_for_cleanup(&run_model).clear();
    for callback in callbacks {
        host.release(callback);
    }
    result.map_err(|_| "GUI_BACKEND")
}

struct PendingEvent {
    callback: u64,
    event: Data,
}

fn deliver_pending(host: HostApi, pending: &mut Vec<PendingEvent>) {
    let pending = std::mem::take(pending);
    if pending.is_empty() {
        let _ = host.pump();
        return;
    }
    for event in pending {
        let _ = host.post(event.callback, event.event);
        let _ = host.pump();
    }
}

struct DesktopApp {
    model: Arc<Mutex<Model>>,
    host: HostApi,
    root_window: Option<u64>,
    textures: HashMap<String, egui::TextureHandle>,
    pending: Vec<PendingEvent>,
    fonts_loaded: bool,
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        if !self.fonts_loaded {
            self.install_system_fallback_fonts(&context);
            self.fonts_loaded = true;
        }
        let model_shared = self.model.clone();
        let Ok(mut model) = lock_model(&model_shared) else {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        let theme = model.nodes.values().find_map(|node| match &node.kind {
            NodeKind::Application { theme, .. } => Some(theme.as_str()),
            _ => None,
        });
        match theme {
            Some("亮色") => context.set_visuals(egui::Visuals::light()),
            Some("暗色") => context.set_visuals(egui::Visuals::dark()),
            _ => {}
        }
        if model.exit_requested {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let windows = model
            .nodes
            .values()
            .filter(|node| matches!(node.kind, NodeKind::Window(_)) && node.common.visible)
            .map(|node| node.id)
            .collect::<Vec<_>>();
        if self.root_window.is_none() {
            self.root_window = windows.first().copied();
        }
        if let Some(root) = self.root_window
            && model.nodes.contains_key(&root)
        {
            self.apply_window_commands(&context, &model, root);
            self.collect_window_events(&context, &mut model, root);
            self.handle_window_close(&context, &model, root);
            self.render_children(ui, &mut model, root);
        }
        for window_id in windows {
            if Some(window_id) == self.root_window {
                continue;
            }
            let Some(Node {
                kind: NodeKind::Window(window),
                common,
                ..
            }) = model.nodes.get(&window_id).cloned()
            else {
                continue;
            };
            let mut builder = egui::ViewportBuilder::default()
                .with_title(window.title)
                .with_inner_size([window.width, window.height])
                .with_min_inner_size([window.minimum_width, window.minimum_height])
                .with_resizable(window.resizable)
                .with_visible(common.visible);
            if let (Some(width), Some(height)) = (window.maximum_width, window.maximum_height) {
                builder = builder.with_max_inner_size([width, height]);
            }
            let viewport_id = egui::ViewportId::from_hash_of(("yanxu-window", window_id));
            context.show_viewport_immediate(viewport_id, builder, |ui, _class| {
                let viewport_context = ui.ctx().clone();
                self.apply_window_commands(&viewport_context, &model, window_id);
                self.collect_window_events(&viewport_context, &mut model, window_id);
                self.handle_window_close(&viewport_context, &model, window_id);
                self.render_children(ui, &mut model, window_id);
            });
        }
        for (timer, callback) in model.due_timers(Instant::now()) {
            if let Ok(node) = model.node(timer) {
                self.pending.push(PendingEvent {
                    callback,
                    event: event_data("定时器", node, None),
                });
            }
        }
        let next_timer = model
            .nodes
            .values()
            .filter_map(|node| match &node.kind {
                NodeKind::Timer(timer) if !timer.cancelled => Some(timer.next),
                _ => None,
            })
            .min();
        if let Some(next) = next_timer {
            context.request_repaint_after(next.saturating_duration_since(Instant::now()));
        }
        if model.repaint_requested {
            model.repaint_requested = false;
            context.request_repaint();
        }
        drop(model);
        deliver_pending(self.host, &mut self.pending);
    }
}

impl DesktopApp {
    fn install_system_fallback_fonts(&self, context: &egui::Context) {
        #[cfg(target_os = "macos")]
        const CANDIDATES: &[&str] = &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ];
        #[cfg(target_os = "windows")]
        const CANDIDATES: &[&str] = &[
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/simhei.ttf",
            "C:/Windows/Fonts/simsun.ttc",
        ];
        #[cfg(target_os = "linux")]
        const CANDIDATES: &[&str] = &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        const CANDIDATES: &[&str] = &[];

        for (index, path) in CANDIDATES.iter().enumerate() {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let name = format!("yanxu-system-fallback-{index}");
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(name.clone());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(name);
            context.set_fonts(fonts);
            break;
        }
    }

    fn apply_window_commands(&self, context: &egui::Context, model: &Model, id: u64) {
        let Ok(node) = model.node(id) else { return };
        let NodeKind::Window(window) = &node.kind else {
            return;
        };
        context.send_viewport_cmd(egui::ViewportCommand::Title(window.title.clone()));
        context.send_viewport_cmd(egui::ViewportCommand::Visible(node.common.visible));
        context.send_viewport_cmd(egui::ViewportCommand::Resizable(window.resizable));
        context.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            window.minimum_width,
            window.minimum_height,
        )));
        if let (Some(width), Some(height)) = (window.maximum_width, window.maximum_height) {
            context.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(
                width, height,
            )));
        }
        context.send_viewport_cmd(egui::ViewportCommand::Maximized(window.maximized));
        context.send_viewport_cmd(egui::ViewportCommand::Minimized(window.minimized));
        context.send_viewport_cmd(egui::ViewportCommand::Fullscreen(window.fullscreen));
        context.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            if window.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));
        if window.centered
            && window.position.is_none()
            && let Some(monitor) = context.input(|input| input.viewport().monitor_size)
        {
            context.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                ((monitor.x - window.width) / 2.0).max(0.0),
                ((monitor.y - window.height) / 2.0).max(0.0),
            )));
        }
        if let Some(bytes) = &window.icon
            && let Ok(image) = image::load_from_memory(bytes)
        {
            let image = image.to_rgba8();
            let width = image.width();
            let height = image.height();
            context.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(
                egui::IconData {
                    rgba: image.into_raw(),
                    width,
                    height,
                },
            ))));
        }
    }

    fn handle_window_close(&mut self, context: &egui::Context, model: &Model, id: u64) {
        if !context.input(|input| input.viewport().close_requested()) {
            return;
        }
        if let Ok(node) = model.node(id)
            && let Some(callback) = node.events.get("关闭")
        {
            self.pending.push(PendingEvent {
                callback: *callback,
                event: event_data("窗口关闭", node, None),
            });
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    fn collect_window_events(&mut self, context: &egui::Context, model: &mut Model, id: u64) {
        let (viewport, dropped_files) =
            context.input(|input| (input.viewport().clone(), input.raw.dropped_files.clone()));
        let Ok(node) = model.node(id).cloned() else {
            return;
        };
        let NodeKind::Window(previous) = &node.kind else {
            return;
        };
        let mut size = None;
        let mut position = None;
        if let Some(rect) = viewport.inner_rect {
            let current = [rect.width(), rect.height()];
            if (current[0] - previous.width).abs() > f32::EPSILON
                || (current[1] - previous.height).abs() > f32::EPSILON
            {
                size = Some(current);
            }
        }
        if let Some(rect) = viewport.outer_rect {
            let current = [rect.min.x, rect.min.y];
            if previous.position != Some(current) {
                position = Some(current);
            }
        }
        let dpi = viewport.native_pixels_per_point.unwrap_or(previous.dpi);
        if let Ok(Node {
            kind: NodeKind::Window(window),
            ..
        }) = model.node_mut(id)
        {
            if let Some([width, height]) = size {
                window.width = width;
                window.height = height;
            }
            if let Some(position) = position {
                window.position = Some(position);
            }
            window.dpi = dpi;
            window.minimized = viewport.minimized.unwrap_or(window.minimized);
            window.maximized = viewport.maximized.unwrap_or(window.maximized);
            window.fullscreen = viewport.fullscreen.unwrap_or(window.fullscreen);
        }
        if let Some([width, height]) = size
            && let Some(callback) = node.events.get("缩放")
        {
            self.pending.push(PendingEvent {
                callback: *callback,
                event: event_data(
                    "窗口缩放",
                    &node,
                    Some(Data::Map(BTreeMap::from([
                        ("宽".into(), Data::Number(width.into())),
                        ("高".into(), Data::Number(height.into())),
                        ("DPI".into(), Data::Number(dpi.into())),
                    ]))),
                ),
            });
        }
        if let Some([x, y]) = position
            && let Some(callback) = node.events.get("移动")
        {
            self.pending.push(PendingEvent {
                callback: *callback,
                event: event_data(
                    "窗口移动",
                    &node,
                    Some(Data::Array(vec![
                        Data::Number(x.into()),
                        Data::Number(y.into()),
                    ])),
                ),
            });
        }
        if !dropped_files.is_empty()
            && let Some(callback) = node.events.get("文件拖放")
        {
            let files = dropped_files
                .into_iter()
                .map(|file| {
                    file.path.map_or_else(
                        || Data::String(file.name),
                        |path| Data::String(path.to_string_lossy().into_owned()),
                    )
                })
                .collect();
            self.pending.push(PendingEvent {
                callback: *callback,
                event: event_data("文件拖放", &node, Some(Data::Array(files))),
            });
        }
    }

    fn render_children(&mut self, ui: &mut egui::Ui, model: &mut Model, parent: u64) {
        let children = model
            .node(parent)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        for child in children {
            self.render_node(ui, model, child);
        }
    }

    fn render_node(&mut self, ui: &mut egui::Ui, model: &mut Model, id: u64) {
        let Ok(node) = model.node(id).cloned() else {
            return;
        };
        if !node.common.visible {
            return;
        }
        match node.kind.clone() {
            NodeKind::Layout(layout) => {
                ui.add_space(layout.padding);
                match layout.kind {
                    LayoutKind::Vertical => {
                        ui.vertical(|ui| self.render_children(ui, model, id));
                    }
                    LayoutKind::Horizontal => {
                        ui.horizontal(|ui| self.render_children(ui, model, id));
                    }
                    LayoutKind::Grid => {
                        egui::Grid::new(("yanxu-grid", id))
                            .num_columns(layout.columns)
                            .spacing([layout.spacing, layout.spacing])
                            .show(ui, |ui| {
                                let children = model
                                    .node(id)
                                    .map(|node| node.children.clone())
                                    .unwrap_or_default();
                                for (index, child) in children.into_iter().enumerate() {
                                    self.render_node(ui, model, child);
                                    if (index + 1) % layout.columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                    LayoutKind::Stack => {
                        ui.scope(|ui| self.render_children(ui, model, id));
                    }
                    LayoutKind::Scroll => {
                        egui::ScrollArea::both().show(ui, |ui| self.render_children(ui, model, id));
                    }
                };
                ui.add_space(layout.padding);
            }
            NodeKind::Control(control) => self.render_control(ui, model, &node, control),
            _ => {}
        }
    }

    fn render_control(
        &mut self,
        ui: &mut egui::Ui,
        model: &mut Model,
        node: &Node,
        mut control: ControlState,
    ) {
        if control.margin > 0.0 {
            ui.add_space(control.margin);
        }
        let width = node.common.width.unwrap_or_else(|| ui.available_width());
        let height = node.common.height.unwrap_or(24.0);
        let response = ui
            .add_enabled_ui(node.common.enabled, |ui| {
                ui.style_mut().override_font_id = Some(egui::FontId::new(
                    control.font_size,
                    if matches!(control.font_family.as_str(), "等宽" | "monospace") {
                        egui::FontFamily::Monospace
                    } else {
                        egui::FontFamily::Proportional
                    },
                ));
                if let Some(color) = control.text_color {
                    ui.visuals_mut().override_text_color = Some(color32(color));
                }
                if let Some(color) = control.background_color {
                    let color = color32(color);
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = color;
                    ui.visuals_mut().widgets.hovered.weak_bg_fill = color;
                    ui.visuals_mut().widgets.active.weak_bg_fill = color;
                    ui.visuals_mut().widgets.open.weak_bg_fill = color;
                }
                if let Some(color) = control.border_color {
                    let stroke = egui::Stroke::new(control.border_width, color32(color));
                    ui.visuals_mut().widgets.inactive.bg_stroke = stroke;
                    ui.visuals_mut().widgets.hovered.bg_stroke = stroke;
                    ui.visuals_mut().widgets.active.bg_stroke = stroke;
                    ui.visuals_mut().widgets.open.bg_stroke = stroke;
                }
                let radius =
                    egui::CornerRadius::same(control.corner_radius.clamp(0.0, 255.0) as u8);
                ui.visuals_mut().widgets.inactive.corner_radius = radius;
                ui.visuals_mut().widgets.hovered.corner_radius = radius;
                ui.visuals_mut().widgets.active.corner_radius = radius;
                ui.visuals_mut().widgets.open.corner_radius = radius;
                match &mut control.kind {
                    ControlKind::Text => {
                        ui.label(egui::RichText::new(&control.text).size(control.font_size))
                    }
                    ControlKind::Button => {
                        ui.add_sized([width, height], egui::Button::new(&control.text))
                    }
                    ControlKind::Input {
                        multiline,
                        placeholder,
                    } => {
                        let edit = if *multiline {
                            egui::TextEdit::multiline(&mut control.text)
                        } else {
                            egui::TextEdit::singleline(&mut control.text)
                        }
                        .hint_text(placeholder.as_str());
                        ui.add_sized(
                            [width, if *multiline { height.max(80.0) } else { height }],
                            edit,
                        )
                    }
                    ControlKind::Checkbox => ui.checkbox(&mut control.selected, &control.text),
                    ControlKind::Radio { group } => {
                        let response = ui
                            .push_id(group.as_str(), |ui| {
                                ui.radio(control.selected, &control.text)
                            })
                            .inner;
                        if response.clicked() {
                            control.selected = true;
                        }
                        response
                    }
                    ControlKind::Select { options } => {
                        let selected = options
                            .get(control.selected_index)
                            .cloned()
                            .unwrap_or_default();
                        egui::ComboBox::from_id_salt(("yanxu-select", node.id))
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (index, option) in options.iter().enumerate() {
                                    ui.selectable_value(&mut control.selected_index, index, option);
                                }
                            })
                            .response
                    }
                    ControlKind::Slider { minimum, maximum } => {
                        ui.add(egui::Slider::new(&mut control.value, *minimum..=*maximum))
                    }
                    ControlKind::Progress => ui.add_sized(
                        [width, height],
                        egui::ProgressBar::new(control.value.clamp(0.0, 1.0) as f32)
                            .text(&control.text),
                    ),
                    ControlKind::Image {
                        bytes,
                        preserve_ratio,
                    } => self.render_image(ui, node.id, bytes, width, height, *preserve_ratio),
                    ControlKind::List { items } => {
                        let previous = control.selected_index;
                        egui::ScrollArea::vertical()
                            .max_height(height.max(80.0))
                            .show(ui, |ui| {
                                for (index, item) in items.iter().enumerate() {
                                    if ui
                                        .selectable_label(index == control.selected_index, item)
                                        .clicked()
                                    {
                                        control.selected_index = index;
                                    }
                                }
                            });
                        let mut response =
                            ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
                        if previous != control.selected_index {
                            response.mark_changed();
                        }
                        response
                    }
                    ControlKind::Separator => ui.separator(),
                    ControlKind::Tabs { tabs } => {
                        ui.horizontal(|ui| {
                            for (index, tab) in tabs.iter().enumerate() {
                                if ui
                                    .selectable_label(index == control.selected_index, tab)
                                    .clicked()
                                {
                                    control.selected_index = index;
                                }
                            }
                        })
                        .response
                    }
                    ControlKind::Menu { items } => {
                        ui.menu_button(&control.text, |ui| {
                            for (index, item) in items.iter().enumerate() {
                                if ui.button(item).clicked() {
                                    control.selected_index = index;
                                    ui.close();
                                }
                            }
                        })
                        .response
                    }
                    ControlKind::Canvas { commands } => {
                        self.render_canvas(ui, node.id, commands, width, height)
                    }
                }
            })
            .inner;
        let mut response = response;
        if !node.common.tooltip.is_empty() {
            response = response.on_hover_text(&node.common.tooltip);
        }
        if !node.common.accessible_description.is_empty() {
            response = response.on_hover_text(&node.common.accessible_description);
        }
        if !node.common.accessible_name.is_empty() {
            let widget_type = match &control.kind {
                ControlKind::Text => egui::WidgetType::Label,
                ControlKind::Button | ControlKind::Menu { .. } => egui::WidgetType::Button,
                ControlKind::Input { .. } => egui::WidgetType::TextEdit,
                ControlKind::Checkbox => egui::WidgetType::Checkbox,
                ControlKind::Radio { .. } => egui::WidgetType::RadioButton,
                ControlKind::Select { .. } => egui::WidgetType::ComboBox,
                ControlKind::Slider { .. } => egui::WidgetType::Slider,
                ControlKind::Progress => egui::WidgetType::ProgressIndicator,
                ControlKind::Image { .. } => egui::WidgetType::Image,
                ControlKind::List { .. } | ControlKind::Tabs { .. } => {
                    egui::WidgetType::SelectableLabel
                }
                _ => egui::WidgetType::Other,
            };
            let label = if node.common.accessible_description.is_empty() {
                node.common.accessible_name.clone()
            } else {
                format!(
                    "{} — {}",
                    node.common.accessible_name, node.common.accessible_description
                )
            };
            response.widget_info(|| {
                egui::WidgetInfo::labeled(widget_type, node.common.enabled, &label)
            });
        }
        if node.common.focus_requested {
            response.request_focus();
        }
        if response.clicked()
            && control.selected
            && let ControlKind::Radio { group } = &control.kind
        {
            for other in model.nodes.values_mut() {
                if other.id == node.id {
                    continue;
                }
                if let NodeKind::Control(ControlState {
                    kind: ControlKind::Radio { group: other_group },
                    selected,
                    ..
                }) = &mut other.kind
                    && other_group == group
                {
                    *selected = false;
                }
            }
        }
        self.collect_response_events(ui.ctx(), model, node, &response, &control);
        let margin = control.margin;
        if let Ok(Node {
            kind: NodeKind::Control(current),
            common,
            ..
        }) = model.node_mut(node.id)
        {
            *current = control;
            common.focus_requested = false;
            common.was_hovered = response.hovered();
            common.was_focused = response.has_focus();
        }
        if margin > 0.0 {
            ui.add_space(margin);
        }
    }

    fn render_image(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        bytes: &[u8],
        width: f32,
        height: f32,
        preserve_ratio: bool,
    ) -> egui::Response {
        let key = format!("image-{id}-{:x}", Sha256::digest(bytes));
        if !bytes.is_empty()
            && !self.textures.contains_key(&key)
            && let Ok(image) = image::load_from_memory(bytes)
        {
            let image = image.to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let texture = ui.ctx().load_texture(
                key.clone(),
                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                egui::TextureOptions::LINEAR,
            );
            self.textures.insert(key.clone(), texture);
        }
        if let Some(texture) = self.textures.get(&key) {
            let image = egui::Image::new(texture);
            if preserve_ratio {
                ui.add(image.max_size(egui::vec2(width, height)))
            } else {
                ui.add(image.fit_to_exact_size(egui::vec2(width, height)))
            }
        } else {
            ui.allocate_response(egui::vec2(width, height), egui::Sense::hover())
        }
    }

    fn render_canvas(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        commands: &[CanvasCommand],
        width: f32,
        height: f32,
    ) -> egui::Response {
        let (response, painter) =
            ui.allocate_painter(egui::vec2(width, height), egui::Sense::click_and_drag());
        let origin = response.rect.min;
        let mut translation = egui::Vec2::ZERO;
        let mut scale = egui::vec2(1.0, 1.0);
        let mut clip = response.rect;
        for (index, command) in commands.iter().enumerate() {
            let point = |point: [f32; 2]| {
                origin + translation + egui::vec2(point[0] * scale.x, point[1] * scale.y)
            };
            match command {
                CanvasCommand::Clear(color) => {
                    painter.rect_filled(response.rect, 0.0, color32(*color));
                }
                CanvasCommand::Line {
                    from,
                    to,
                    color,
                    width,
                } => {
                    painter.line_segment(
                        [point(*from), point(*to)],
                        egui::Stroke::new(*width, color32(*color)),
                    );
                }
                CanvasCommand::Rectangle {
                    minimum,
                    maximum,
                    color,
                    stroke,
                    stroke_width,
                    radius,
                } => {
                    painter.rect(
                        egui::Rect::from_two_pos(point(*minimum), point(*maximum)),
                        *radius,
                        color32(*color),
                        egui::Stroke::new(*stroke_width, color32(*stroke)),
                        egui::StrokeKind::Middle,
                    );
                }
                CanvasCommand::Circle {
                    center,
                    radius,
                    color,
                    stroke,
                    stroke_width,
                } => {
                    painter.circle(
                        point(*center),
                        *radius * scale.x.min(scale.y),
                        color32(*color),
                        egui::Stroke::new(*stroke_width, color32(*stroke)),
                    );
                }
                CanvasCommand::Text {
                    position,
                    text,
                    color,
                    size,
                } => {
                    painter.text(
                        point(*position),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::proportional(*size),
                        color32(*color),
                    );
                }
                CanvasCommand::Image {
                    minimum,
                    maximum,
                    bytes,
                } => {
                    let key = format!("canvas-{id}-{index}-{:x}", Sha256::digest(bytes));
                    if !self.textures.contains_key(&key)
                        && let Ok(image) = image::load_from_memory(bytes)
                    {
                        let image = image.to_rgba8();
                        let size = [image.width() as usize, image.height() as usize];
                        self.textures.insert(
                            key.clone(),
                            ui.ctx().load_texture(
                                key.clone(),
                                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                    }
                    if let Some(texture) = self.textures.get(&key) {
                        painter.image(
                            texture.id(),
                            egui::Rect::from_two_pos(point(*minimum), point(*maximum))
                                .intersect(clip),
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                }
                CanvasCommand::Clip { minimum, maximum } => {
                    clip = egui::Rect::from_two_pos(point(*minimum), point(*maximum))
                        .intersect(response.rect)
                }
                CanvasCommand::Transform {
                    translation: next,
                    scale: next_scale,
                } => {
                    translation = egui::vec2(next[0], next[1]);
                    scale = egui::vec2(next_scale[0], next_scale[1]);
                }
            }
        }
        response
    }

    fn collect_response_events(
        &mut self,
        context: &egui::Context,
        model: &Model,
        node: &Node,
        response: &egui::Response,
        control: &ControlState,
    ) {
        let mut emit = |name: &str, extra: Option<Data>| {
            if let Some(callback) = node.events.get(name) {
                self.pending.push(PendingEvent {
                    callback: *callback,
                    event: event_data(name, node, extra),
                });
            }
        };
        if response.clicked() {
            emit("点击", None);
        }
        if response.double_clicked() {
            emit("双击", None);
        }
        if response.changed() {
            emit(
                "变化",
                Some(Data::Map(BTreeMap::from([
                    ("文字".into(), Data::String(control.text.clone())),
                    ("值".into(), Data::Number(control.value)),
                    ("选中".into(), Data::Bool(control.selected)),
                    (
                        "当前项".into(),
                        Data::Integer(control.selected_index as i64),
                    ),
                ]))),
            );
        }
        if response.hovered() && !node.common.was_hovered {
            emit("鼠标进入", None);
        }
        if !response.hovered() && node.common.was_hovered {
            emit("鼠标离开", None);
        }
        if response.has_focus() && !node.common.was_focused {
            emit("获得焦点", None);
        }
        if !response.has_focus() && node.common.was_focused {
            emit("失去焦点", None);
        }
        if response.dragged() {
            emit("鼠标移动", pointer_event(context));
        }
        if response.drag_started() {
            emit("鼠标按下", pointer_event(context));
        }
        if response.drag_stopped() {
            emit("鼠标释放", pointer_event(context));
        }
        if response.hovered() {
            let scroll = context.input(|input| input.smooth_scroll_delta());
            if scroll != egui::Vec2::ZERO {
                emit(
                    "滚轮",
                    Some(Data::Array(vec![
                        Data::Number(scroll.x.into()),
                        Data::Number(scroll.y.into()),
                    ])),
                );
            }
        }
        if response.has_focus() {
            for input in context.input(|input| input.events.clone()) {
                match input {
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => emit(
                        if pressed {
                            "键盘按下"
                        } else {
                            "键盘释放"
                        },
                        Some(Data::Map(BTreeMap::from([
                            ("按键".into(), Data::String(format!("{key:?}"))),
                            ("修饰键".into(), Data::String(format!("{modifiers:?}"))),
                        ]))),
                    ),
                    egui::Event::Text(text) => emit("文字变化", Some(Data::String(text))),
                    _ => {}
                }
            }
        }
        let _ = model;
    }
}

fn pointer_event(context: &egui::Context) -> Option<Data> {
    context
        .input(|input| input.pointer.hover_pos())
        .map(|position| {
            Data::Array(vec![
                Data::Number(position.x.into()),
                Data::Number(position.y.into()),
            ])
        })
}

fn color32(color: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

fn event_data(kind: &str, node: &Node, details: Option<Data>) -> Data {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    let mut event = BTreeMap::from([
        ("类型".into(), Data::String(kind.into())),
        (
            "目标".into(),
            Data::Integer(if node.public_handle == 0 {
                node.id
            } else {
                node.public_handle
            } as i64),
        ),
        ("时间".into(), Data::Integer(milliseconds)),
        ("横坐标".into(), Data::Number(0.0)),
        ("纵坐标".into(), Data::Number(0.0)),
        ("按键".into(), Data::String(String::new())),
        ("修饰键".into(), Data::String(String::new())),
        ("鼠标按钮".into(), Data::String(String::new())),
        ("文字".into(), Data::String(String::new())),
        ("是否已处理".into(), Data::Bool(false)),
        ("详情".into(), Data::Nil),
    ]);
    if let Some(details) = details {
        match details {
            Data::Array(position) if position.len() == 2 => {
                event.insert("横坐标".into(), position[0].clone());
                event.insert("纵坐标".into(), position[1].clone());
            }
            Data::String(text) => {
                event.insert("文字".into(), Data::String(text));
            }
            Data::Map(details) => {
                event.extend(details);
            }
            details => {
                event.insert("详情".into(), details);
            }
        }
    }
    Data::Map(event)
}

fn custom_event_data(kind: &str, node: &Node, payload: Data) -> Data {
    event_data(
        kind,
        node,
        Some(Data::Map(BTreeMap::from([("详情".into(), payload)]))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct DeliveryTrace {
        steps: Vec<String>,
    }

    unsafe extern "C" fn trace_post(
        context: *mut c_void,
        callback: u64,
        _arguments: *const crate::abi::Value,
        _count: usize,
        _error: *mut NativeError,
    ) -> i32 {
        let trace = unsafe { &mut *context.cast::<DeliveryTrace>() };
        trace.steps.push(format!("post:{callback}"));
        abi::OK
    }

    unsafe extern "C" fn trace_pump(
        context: *mut c_void,
        _maximum_events: usize,
        _error: *mut NativeError,
    ) -> i32 {
        let trace = unsafe { &mut *context.cast::<DeliveryTrace>() };
        trace.steps.push("pump".into());
        abi::OK
    }

    struct TimerHostTrace {
        resource: *mut c_void,
        released: Vec<u64>,
    }

    unsafe extern "C" fn trace_release(context: *mut c_void, callback: u64) -> i32 {
        let trace = unsafe { &mut *context.cast::<TimerHostTrace>() };
        trace.released.push(callback);
        abi::OK
    }

    unsafe extern "C" fn trace_resource_get(
        context: *mut c_void,
        _handle: u64,
        output: *mut *mut c_void,
    ) -> i32 {
        let trace = unsafe { &*context.cast::<TimerHostTrace>() };
        if output.is_null() || trace.resource.is_null() {
            return abi::ERROR;
        }
        unsafe { *output = trace.resource };
        abi::OK
    }

    fn string_array(values: &[&str]) -> Data {
        Data::Array(
            values
                .iter()
                .map(|value| Data::String((*value).into()))
                .collect(),
        )
    }

    #[test]
    fn headless_model_supports_every_public_control_layout_and_style_shape() {
        for layout in ["纵向", "横向", "网格", "层叠", "滚动"] {
            assert!(layout_kind(layout).is_ok(), "missing layout {layout}");
        }

        let mut config = BTreeMap::from([
            ("文字".into(), Data::String("中文🙂".into())),
            ("占位".into(), Data::String("请输入".into())),
            ("组".into(), Data::String("选择".into())),
            ("选项".into(), string_array(&["甲", "乙"])),
            ("项目".into(), string_array(&["一", "二"])),
            ("标签".into(), string_array(&["页一", "页二"])),
            ("文字颜色".into(), Data::String("#102030ff".into())),
            (
                "背景颜色".into(),
                Data::Array(vec![
                    Data::Integer(1),
                    Data::Integer(2),
                    Data::Integer(3),
                    Data::Integer(4),
                ]),
            ),
            ("边框颜色".into(), Data::String("#ffffff".into())),
            ("字号".into(), Data::Number(18.0)),
            ("字重".into(), Data::Integer(700)),
            ("内边距".into(), Data::Integer(8)),
            ("外边距".into(), Data::Integer(4)),
        ]);
        for kind in [
            "文字",
            "按钮",
            "输入框",
            "多行输入框",
            "复选框",
            "单选框",
            "下拉选择",
            "滑块",
            "进度条",
            "图片",
            "列表",
            "分隔线",
            "标签页",
            "菜单",
            "画布",
        ] {
            let control = create_control(kind, &config).unwrap();
            assert_eq!(control.text, "");
            let mut control = control;
            apply_control_config(&mut control, &config).unwrap();
            assert_eq!(control.text, "中文🙂");
            assert_eq!(control.font_size, 18.0);
            assert_eq!(control.font_weight, 700);
            assert_eq!(control.text_color, Some([0x10, 0x20, 0x30, 0xff]));
        }

        config.insert("类型".into(), Data::String("线段".into()));
        config.insert(
            "起点".into(),
            Data::Array(vec![Data::Integer(0), Data::Integer(1)]),
        );
        config.insert(
            "终点".into(),
            Data::Array(vec![Data::Integer(2), Data::Integer(3)]),
        );
        assert!(matches!(
            canvas_command(&config).unwrap(),
            CanvasCommand::Line { .. }
        ));
    }

    #[test]
    fn event_objects_have_the_stable_complete_public_shape() {
        let node = Node {
            id: 7,
            public_handle: 99,
            parent: None,
            children: Vec::new(),
            common: Common::default(),
            kind: NodeKind::Control(ControlState::new(ControlKind::Button)),
            events: BTreeMap::new(),
        };
        let Data::Map(event) = event_data(
            "鼠标移动",
            &node,
            Some(Data::Array(vec![Data::Number(12.5), Data::Number(21.0)])),
        ) else {
            panic!("event must be a map");
        };
        for field in [
            "类型",
            "目标",
            "时间",
            "横坐标",
            "纵坐标",
            "按键",
            "修饰键",
            "鼠标按钮",
            "文字",
            "是否已处理",
            "详情",
        ] {
            assert!(event.contains_key(field), "missing event field {field}");
        }
        assert_eq!(event["目标"], Data::Integer(99));
        assert_eq!(event["横坐标"], Data::Number(12.5));

        let payload = Data::Map(BTreeMap::from([
            ("类型".into(), Data::String("伪造类型".into())),
            ("目标".into(), Data::Integer(-1)),
        ]));
        let Data::Map(custom) = custom_event_data("业务事件", &node, payload.clone()) else {
            panic!("custom event must be a map");
        };
        assert_eq!(custom["类型"], Data::String("业务事件".into()));
        assert_eq!(custom["目标"], Data::Integer(99));
        assert_eq!(custom["详情"], payload);
        assert_eq!(
            event_name(&Data::String(String::new())),
            Err("GUI_EVENT_NAME")
        );
        assert_eq!(
            event_name(&Data::String("x".repeat(257))),
            Err("GUI_EVENT_NAME")
        );
    }

    #[test]
    fn pending_events_are_pumped_before_the_next_callback_is_posted() {
        let mut trace = DeliveryTrace::default();
        let host = HostApi(NativeHost {
            abi_version: abi::ABI,
            struct_size: std::mem::size_of::<NativeHost>(),
            context: (&raw mut trace).cast(),
            callback_retain: None,
            callback_release: None,
            callback_post: Some(trace_post),
            wake: None,
            pump: Some(trace_pump),
            has_permission: None,
            resource_get: None,
            event_loop_id: 1,
            owner_thread_token: 1,
        });
        let mut pending = vec![
            PendingEvent {
                callback: 10,
                event: Data::Nil,
            },
            PendingEvent {
                callback: 20,
                event: Data::Nil,
            },
        ];

        deliver_pending(host, &mut pending);

        assert!(pending.is_empty());
        assert_eq!(trace.steps, ["post:10", "pump", "post:20", "pump"]);
        deliver_pending(host, &mut pending);
        assert_eq!(trace.steps.last().map(String::as_str), Some("pump"));
    }

    #[test]
    fn cancelling_a_timer_releases_its_retained_callback_exactly_once() {
        let model = Arc::new(Mutex::new(Model::default()));
        let (app, timer) = {
            let mut model = model.lock().expect("fresh model");
            let app = model
                .create(
                    None,
                    NodeKind::Application {
                        title: "测试".into(),
                        theme: "系统".into(),
                    },
                )
                .expect("create application");
            let timer = model
                .create(
                    Some(app),
                    NodeKind::Timer(TimerState {
                        interval: Duration::from_millis(10),
                        repeating: true,
                        next: Instant::now() + Duration::from_secs(1),
                        callback: Some(42),
                        cancelled: false,
                    }),
                )
                .expect("create timer");
            (app, timer)
        };
        let mut trace = TimerHostTrace {
            resource: std::ptr::null_mut(),
            released: Vec::new(),
        };
        let host = HostApi(NativeHost {
            abi_version: abi::ABI,
            struct_size: std::mem::size_of::<NativeHost>(),
            context: (&raw mut trace).cast(),
            callback_retain: None,
            callback_release: Some(trace_release),
            callback_post: None,
            wake: None,
            pump: None,
            has_permission: None,
            resource_get: Some(trace_resource_get),
            event_loop_id: 1,
            owner_thread_token: 1,
        });
        let mut resource = GuiResource {
            model: Arc::clone(&model),
            kind: ResourceKind::Timer,
            id: timer,
            host,
            cleaned: AtomicBool::new(false),
        };
        trace.resource = (&raw mut resource).cast();
        let arguments = [
            Data::Resource(7),
            Data::String("取消".into()),
            Data::Bool(true),
        ];

        assert!(matches!(
            unsafe { call(Operation::SetProperty, &arguments, host) },
            Ok(Output::Value(Data::Nil))
        ));
        assert!(matches!(
            unsafe { call(Operation::SetProperty, &arguments, host) },
            Ok(Output::Value(Data::Nil))
        ));

        assert_eq!(trace.released, [42]);
        let model = lock_model(&model).expect("model remains healthy");
        let NodeKind::Timer(timer_state) =
            &model.node(timer).expect("timer remains queryable").kind
        else {
            panic!("expected timer node");
        };
        assert!(timer_state.cancelled);
        assert_eq!(timer_state.callback, None);
        assert_eq!(
            model.node(app).expect("application remains live").children,
            [timer]
        );
    }

    #[test]
    fn poisoned_models_fail_calls_but_still_allow_idempotent_cleanup() {
        let model = Arc::new(Mutex::new(Model::default()));
        let id = model
            .lock()
            .expect("fresh model")
            .create(
                None,
                NodeKind::Application {
                    title: "测试".into(),
                    theme: "系统".into(),
                },
            )
            .expect("create application");
        let poisoned = Arc::clone(&model);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned.lock().expect("fresh model");
            panic!("poison model for cleanup test");
        }));
        assert!(panic.is_err());

        let resource = GuiResource {
            model: Arc::clone(&model),
            kind: ResourceKind::Application,
            id,
            host: HostApi(NativeHost {
                abi_version: abi::ABI,
                struct_size: std::mem::size_of::<NativeHost>(),
                context: std::ptr::null_mut(),
                callback_retain: None,
                callback_release: None,
                callback_post: None,
                wake: None,
                pump: None,
                has_permission: None,
                resource_get: None,
                event_loop_id: 1,
                owner_thread_token: 1,
            }),
            cleaned: AtomicBool::new(false),
        };

        assert_eq!(
            set_property(&resource, "名称", &Data::String("新名称".into())),
            Err("GUI_BACKEND_STATE")
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| resource.cleanup())).is_ok()
        );
        assert!(lock_model_for_cleanup(&model).node(id).is_err());
        resource.cleanup();
    }

    #[test]
    fn unsafe_sizes_colors_and_canvas_commands_are_rejected() {
        let bad_window = BTreeMap::from([
            ("宽".into(), Data::Integer(100)),
            ("最小宽".into(), Data::Integer(101)),
        ]);
        assert_eq!(
            apply_window_config(&mut WindowState::default(), &bad_window),
            Err("GUI_WINDOW_SIZE")
        );
        assert!(parse_hex_color("not-a-color").is_err());
        assert!(canvas_command(&BTreeMap::new()).is_err());
        assert!(create_control("不存在", &BTreeMap::new()).is_err());
    }
}
