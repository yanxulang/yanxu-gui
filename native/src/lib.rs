mod abi;
mod backend;
mod bridge;
mod model;

use abi::*;
use backend::{HostApi, Operation, Output};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::OnceLock;

static MODULE: OnceLock<usize> = OnceLock::new();
static MODULE_NAME: &[u8] = b"yanxu-gui";
static ERROR_MESSAGE: &[u8] = b"yanxu-gui rejected the operation";
static PANIC_CODE: &[u8] = b"GUI_BACKEND_PANIC";
static PANIC_MESSAGE: &[u8] = b"panic isolated inside yanxu-gui backend";

static FUNCTIONS: &[(&[u8], Operation)] = &[
    ("应用创建".as_bytes(), Operation::CreateApplication),
    ("窗口创建".as_bytes(), Operation::CreateWindow),
    ("布局创建".as_bytes(), Operation::CreateLayout),
    ("控件创建".as_bytes(), Operation::CreateControl),
    ("属性设置".as_bytes(), Operation::SetProperty),
    ("属性获取".as_bytes(), Operation::GetProperty),
    ("事件绑定".as_bytes(), Operation::BindEvent),
    ("显示".as_bytes(), Operation::Show),
    ("隐藏".as_bytes(), Operation::Hide),
    ("关闭".as_bytes(), Operation::Close),
    ("运行".as_bytes(), Operation::Run),
    ("退出".as_bytes(), Operation::Exit),
    ("定时器创建".as_bytes(), Operation::CreateTimer),
    ("主题设置".as_bytes(), Operation::SetTheme),
    ("剪贴板读取".as_bytes(), Operation::ClipboardRead),
    ("剪贴板写入".as_bytes(), Operation::ClipboardWrite),
    ("文件对话框".as_bytes(), Operation::Dialog),
    ("画布命令".as_bytes(), Operation::CanvasCommand),
    ("自定义事件".as_bytes(), Operation::EmitCustom),
    ("调试快照".as_bytes(), Operation::DebugSnapshot),
];

static RESOURCE_TYPES: &[&[u8]] = &[
    b"yanxu.gui.application",
    b"yanxu.gui.window",
    b"yanxu.gui.layout",
    b"yanxu.gui.control",
    b"yanxu.gui.timer",
];

#[unsafe(no_mangle)]
pub extern "C" fn yanxu_native_module_v2() -> *const NativeModule {
    *MODULE.get_or_init(|| {
        let functions = Box::leak(
            FUNCTIONS
                .iter()
                .map(|(name, operation)| NativeFunction {
                    name: name.as_ptr(),
                    name_length: name.len(),
                    context: (*operation as usize) as *mut c_void,
                    call: Some(dispatch),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let resource_types = Box::leak(
            RESOURCE_TYPES
                .iter()
                .map(|name| name.as_ptr())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let resource_lengths = Box::leak(
            RESOURCE_TYPES
                .iter()
                .map(|name| name.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        Box::into_raw(Box::new(NativeModule {
            abi_version: ABI,
            struct_size: std::mem::size_of::<NativeModule>(),
            name: MODULE_NAME.as_ptr(),
            name_length: MODULE_NAME.len(),
            functions: functions.as_ptr(),
            function_count: functions.len(),
            constants: ptr::null(),
            constant_count: 0,
            resource_types: resource_types.as_ptr(),
            resource_type_lengths: resource_lengths.as_ptr(),
            resource_type_count: resource_types.len(),
            free_value: Some(bridge::free_value),
            capabilities: 0b111_1111,
        })) as usize
    }) as *const NativeModule
}

unsafe extern "C" fn dispatch(
    context: *mut c_void,
    arguments: *const Value,
    count: usize,
    host: *const NativeHost,
    output: *mut Value,
    error: *mut NativeError,
) -> i32 {
    if output.is_null() || host.is_null() {
        return fail(error, "GUI_HOST_ABI");
    }
    let Some(operation) = Operation::from_context(context) else {
        return fail(error, "GUI_FUNCTION");
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let arguments = unsafe { bridge::decode_arguments(arguments, count) }?;
        let host = HostApi(unsafe { *host });
        unsafe { backend::call(operation, &arguments, host) }
    }));
    match result {
        Ok(Ok(Output::Value(value))) => {
            unsafe { *output = bridge::encode_data(value) };
            OK
        }
        Ok(Ok(Output::Resource(resource))) => {
            let raw = Box::into_raw(resource.resource).cast::<c_void>();
            let descriptor = Box::new(NativeResource {
                struct_size: std::mem::size_of::<NativeResource>(),
                resource: raw,
                type_name: resource.type_name.as_ptr(),
                type_name_length: resource.type_name.len(),
                parent: resource.parent,
                drop_resource: Some(backend::drop_gui_resource),
            });
            unsafe {
                *output = Value {
                    kind: RESOURCE,
                    data: ValueData {
                        resource: Box::into_raw(descriptor),
                    },
                    ..Value::default()
                }
            };
            OK
        }
        Ok(Err(code)) => fail(error, code),
        Err(_) => {
            if let Some(error) = unsafe { error.as_mut() } {
                *error = NativeError {
                    code: PANIC_CODE.as_ptr(),
                    code_length: PANIC_CODE.len(),
                    message: PANIC_MESSAGE.as_ptr(),
                    message_length: PANIC_MESSAGE.len(),
                };
            }
            ERROR
        }
    }
}

fn fail(error: *mut NativeError, code: &'static str) -> i32 {
    if let Some(error) = unsafe { error.as_mut() } {
        *error = NativeError {
            code: code.as_ptr(),
            code_length: code.len(),
            message: ERROR_MESSAGE.as_ptr(),
            message_length: ERROR_MESSAGE.len(),
        };
    }
    ERROR
}
