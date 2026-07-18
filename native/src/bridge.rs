use crate::abi::*;
use crate::model::Data;
use std::collections::BTreeMap;
use std::ptr;

const MAX_VALUE_ELEMENTS: usize = 65_536;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct DecodeBudget {
    elements: usize,
    bytes: usize,
}

impl DecodeBudget {
    fn add_element(&mut self) -> Result<(), &'static str> {
        self.elements = self.elements.checked_add(1).ok_or("GUI_VALUE_LIMIT")?;
        (self.elements <= MAX_VALUE_ELEMENTS)
            .then_some(())
            .ok_or("GUI_VALUE_LIMIT")
    }

    fn add_bytes(&mut self, bytes: usize) -> Result<(), &'static str> {
        self.bytes = self.bytes.checked_add(bytes).ok_or("GUI_VALUE_LIMIT")?;
        (self.bytes <= MAX_TOTAL_BYTES)
            .then_some(())
            .ok_or("GUI_VALUE_LIMIT")
    }
}

pub unsafe fn decode_arguments(
    arguments: *const Value,
    count: usize,
) -> Result<Vec<Data>, &'static str> {
    if count > MAX_VALUE_ELEMENTS || (count > 0 && arguments.is_null()) {
        return Err("GUI_VALUE_LIMIT");
    }
    let arguments = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(arguments, count) }
    };
    let mut budget = DecodeBudget::default();
    arguments
        .iter()
        .map(|value| unsafe { decode_value(value, 0, &mut budget) })
        .collect()
}

unsafe fn decode_value(
    value: &Value,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Data, &'static str> {
    if depth > 64 {
        return Err("GUI_VALUE_LIMIT");
    }
    budget.add_element()?;
    Ok(match value.kind {
        NULL => Data::Nil,
        BOOL => Data::Bool(value.flags & FLAG_TRUE != 0),
        INTEGER => Data::Integer(unsafe { value.data.integer }),
        NUMBER => {
            let number = unsafe { value.data.number };
            if !number.is_finite() {
                return Err("GUI_VALUE_TYPE");
            }
            Data::Number(number)
        }
        STRING => Data::String(
            String::from_utf8(unsafe { copy_bytes(value, 4 * 1024 * 1024, budget) }?)
                .map_err(|_| "GUI_VALUE_UTF8")?,
        ),
        BYTES => Data::Bytes(unsafe { copy_bytes(value, MAX_TOTAL_BYTES, budget) }?),
        ARRAY => {
            let count = usize::try_from(value.length).map_err(|_| "GUI_VALUE_LIMIT")?;
            let values = unsafe { value_slice(value.data.items, count) }?;
            Data::Array(
                values
                    .iter()
                    .map(|value| unsafe { decode_value(value, depth + 1, budget) })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        MAP => {
            let count = usize::try_from(value.length).map_err(|_| "GUI_VALUE_LIMIT")?;
            let item_count = count.checked_mul(2).ok_or("GUI_VALUE_LIMIT")?;
            let values = unsafe { value_slice(value.data.items, item_count) }?;
            let mut map = BTreeMap::new();
            for pair in values.chunks_exact(2) {
                let Data::String(key) = (unsafe { decode_value(&pair[0], depth + 1, budget) })?
                else {
                    return Err("GUI_VALUE_TYPE");
                };
                let value = unsafe { decode_value(&pair[1], depth + 1, budget) }?;
                if map.insert(key, value).is_some() {
                    return Err("GUI_VALUE_TYPE");
                }
            }
            Data::Map(map)
        }
        RESOURCE if value.flags & FLAG_RESOURCE_HANDLE != 0 => {
            Data::Resource(unsafe { value.data.handle })
        }
        CALLBACK => Data::Callback(unsafe { value.data.handle }),
        _ => return Err("GUI_VALUE_TYPE"),
    })
}

unsafe fn copy_bytes(
    value: &Value,
    limit: usize,
    budget: &mut DecodeBudget,
) -> Result<Vec<u8>, &'static str> {
    let length = usize::try_from(value.length).map_err(|_| "GUI_VALUE_LIMIT")?;
    if length > limit {
        return Err("GUI_VALUE_LIMIT");
    }
    budget.add_bytes(length)?;
    if length == 0 {
        return Ok(Vec::new());
    }
    let pointer = unsafe { value.data.bytes };
    if pointer.is_null() {
        return Err("GUI_VALUE_TYPE");
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec())
}

unsafe fn value_slice<'a>(
    pointer: *const Value,
    length: usize,
) -> Result<&'a [Value], &'static str> {
    if length > MAX_VALUE_ELEMENTS {
        return Err("GUI_VALUE_LIMIT");
    }
    if length == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err("GUI_VALUE_TYPE");
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

pub fn encode_data(data: Data) -> Value {
    match data {
        Data::Nil => Value::default(),
        Data::Bool(value) => Value {
            kind: BOOL,
            flags: if value { FLAG_TRUE } else { 0 },
            ..Value::default()
        },
        Data::Integer(value) => Value {
            kind: INTEGER,
            data: ValueData { integer: value },
            ..Value::default()
        },
        Data::Number(value) => Value {
            kind: NUMBER,
            data: ValueData { number: value },
            ..Value::default()
        },
        Data::String(value) => encode_bytes(STRING, value.into_bytes()),
        Data::Bytes(value) => encode_bytes(BYTES, value),
        Data::Array(values) => {
            encode_children(ARRAY, values.into_iter().map(encode_data).collect(), false)
        }
        Data::Map(values) => {
            let length = values.len();
            let mut children = Vec::with_capacity(length.saturating_mul(2));
            for (key, value) in values {
                children.push(encode_data(Data::String(key)));
                children.push(encode_data(value));
            }
            encode_children(MAP, children, true)
        }
        Data::Resource(handle) => Value {
            kind: RESOURCE,
            flags: FLAG_RESOURCE_HANDLE,
            data: ValueData { handle },
            ..Value::default()
        },
        Data::Callback(handle) => Value {
            kind: CALLBACK,
            data: ValueData { handle },
            ..Value::default()
        },
    }
}

fn encode_bytes(kind: u32, bytes: Vec<u8>) -> Value {
    if bytes.is_empty() {
        return Value {
            kind,
            ..Value::default()
        };
    }
    let bytes = bytes.into_boxed_slice();
    let length = bytes.len() as u64;
    let pointer = Box::into_raw(bytes) as *mut u8;
    Value {
        kind,
        length,
        data: ValueData { bytes: pointer },
        ..Value::default()
    }
}

fn encode_children(kind: u32, children: Vec<Value>, map: bool) -> Value {
    let logical_length = if map {
        children.len() / 2
    } else {
        children.len()
    };
    if children.is_empty() {
        return Value {
            kind,
            ..Value::default()
        };
    }
    let children = children.into_boxed_slice();
    let pointer = Box::into_raw(children) as *mut Value;
    Value {
        kind,
        length: logical_length as u64,
        data: ValueData { items: pointer },
        ..Value::default()
    }
}

pub unsafe extern "C" fn free_value(value: *mut Value) {
    let Some(value) = (unsafe { value.as_mut() }) else {
        return;
    };
    unsafe { free_value_inner(value) };
    *value = Value::default();
}

unsafe fn free_value_inner(value: &mut Value) {
    match value.kind {
        STRING | BYTES => {
            let length = usize::try_from(value.length).unwrap_or(0);
            let pointer = unsafe { value.data.bytes as *mut u8 };
            if length > 0 && !pointer.is_null() {
                drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(pointer, length)) });
            }
        }
        ARRAY | MAP | ERROR_VALUE => {
            let logical = usize::try_from(value.length).unwrap_or(0);
            let length = if value.kind == MAP {
                logical.saturating_mul(2)
            } else {
                logical
            };
            let pointer = unsafe { value.data.items as *mut Value };
            if length > 0 && !pointer.is_null() {
                let mut values =
                    unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(pointer, length)) };
                for value in &mut values {
                    unsafe { free_value_inner(value) };
                }
            }
        }
        RESOURCE if value.flags & FLAG_RESOURCE_HANDLE == 0 => {
            let pointer = unsafe { value.data.resource };
            if !pointer.is_null() {
                let descriptor = unsafe { Box::from_raw(pointer) };
                if !descriptor.resource.is_null()
                    && let Some(drop_resource) = descriptor.drop_resource
                {
                    unsafe { drop_resource(descriptor.resource) };
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_resource_handles_are_not_freed_as_owned_descriptors() {
        let handle = u64::MAX - 1;
        let mut value = encode_data(Data::Resource(handle));

        assert_eq!(value.kind, RESOURCE);
        assert_eq!(value.flags, FLAG_RESOURCE_HANDLE);
        assert_eq!(unsafe { value.data.handle }, handle);

        unsafe { free_value(&mut value) };

        assert_eq!(value.kind, NULL);
        assert_eq!(value.flags, 0);
        assert_eq!(value.length, 0);
    }

    #[test]
    fn aggregate_element_limit_applies_across_nested_values() {
        let children = vec![Value::default(); MAX_VALUE_ELEMENTS];
        let root = Value {
            kind: ARRAY,
            length: children.len() as u64,
            data: ValueData {
                items: children.as_ptr(),
            },
            ..Value::default()
        };

        assert_eq!(
            unsafe { decode_arguments(&raw const root, 1) },
            Err("GUI_VALUE_LIMIT")
        );
    }

    #[test]
    fn aggregate_byte_limit_cannot_be_bypassed_with_multiple_values() {
        let bytes = vec![0_u8; MAX_TOTAL_BYTES / 2 + 1];
        let value = Value {
            kind: BYTES,
            length: bytes.len() as u64,
            data: ValueData {
                bytes: bytes.as_ptr(),
            },
            ..Value::default()
        };
        let arguments = [value, value];

        assert_eq!(
            unsafe { decode_arguments(arguments.as_ptr(), arguments.len()) },
            Err("GUI_VALUE_LIMIT")
        );
    }
}
