use super::*;

impl TryFrom<StackValue> for StackItem {
    type Error = VmError;

    fn try_from(value: StackValue) -> VmResult<Self> {
        /// A compound whose elements are being converted.
        enum Frame {
            /// An `Array` or `Struct` under construction; `structure` picks
            /// which one.
            List {
                structure: bool,
                source: std::vec::IntoIter<StackValue>,
                items: Vec<StackItem>,
            },
            /// A `Map` under construction; elements arrive as key, value, ...
            Map {
                source: std::vec::IntoIter<StackValue>,
                entries: VmOrderedDictionary<StackItem, StackItem>,
                pending_key: Option<StackItem>,
            },
        }

        /// Converts an item that has no children of its own.
        fn convert_leaf(value: &StackValue) -> VmResult<Option<StackItem>> {
            match value {
                StackValue::Integer(value) => Ok(Some(StackItem::from_i64(*value))),
                StackValue::BigInteger(bytes) => Ok(Some(StackItem::from_int(
                    BigInt::from_signed_bytes_le(bytes),
                ))),
                StackValue::ByteString(bytes) => {
                    Ok(Some(StackItem::from_byte_string(bytes.clone())))
                }
                StackValue::Buffer(bytes) => Ok(Some(StackItem::from_buffer(bytes.clone()))),
                StackValue::Boolean(value) => Ok(Some(StackItem::from_bool(*value))),
                StackValue::Null => Ok(Some(StackItem::Null)),
                StackValue::Pointer(_) | StackValue::Interop(_) | StackValue::Iterator(_) => {
                    Err(VmError::invalid_operation_msg(format!(
                        "Cannot convert {value:?} into neo-vm StackItem without host runtime identity"
                    )))
                }
                _ => Ok(None),
            }
        }

        /// Opens the frame of a compound value.
        fn open(value: StackValue) -> Frame {
            match value {
                StackValue::Array(items) => Frame::List {
                    structure: false,
                    items: Vec::with_capacity(items.len()),
                    source: items.into_iter(),
                },
                StackValue::Struct(items) => Frame::List {
                    structure: true,
                    items: Vec::with_capacity(items.len()),
                    source: items.into_iter(),
                },
                StackValue::Map(entries) => {
                    let capacity = entries.len();
                    let mut source = Vec::with_capacity(capacity * 2);
                    for (key, value) in entries {
                        source.push(key);
                        source.push(value);
                    }
                    Frame::Map {
                        source: source.into_iter(),
                        entries: VmOrderedDictionary::with_capacity(capacity),
                        pending_key: None,
                    }
                }
                _ => unreachable!("only compound values are opened"),
            }
        }

        fn next_child(frame: &mut Frame) -> Option<StackValue> {
            match frame {
                Frame::List { source, .. } | Frame::Map { source, .. } => source.next(),
            }
        }

        fn accept(frame: &mut Frame, item: StackItem) {
            match frame {
                Frame::List { items, .. } => items.push(item),
                Frame::Map {
                    entries,
                    pending_key,
                    ..
                } => match pending_key.take() {
                    None => *pending_key = Some(item),
                    Some(key) => {
                        let _ = entries.insert(key, item);
                    }
                },
            }
        }

        fn close(frame: Frame) -> StackItem {
            match frame {
                Frame::List {
                    structure, items, ..
                } => {
                    if structure {
                        StackItem::from_struct(items)
                    } else {
                        StackItem::from_array(items)
                    }
                }
                Frame::Map { entries, .. } => StackItem::from_map(entries),
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(value);
        let mut converted = StackItem::Null;

        loop {
            if let Some(value) = pending.take() {
                if let Some(item) = convert_leaf(&value)? {
                    converted = item;
                } else {
                    let mut frame = open(value);
                    let first = next_child(&mut frame);
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            converted = close(frame);
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return Ok(converted);
            }
            let item = std::mem::replace(&mut converted, StackItem::Null);
            accept(frames.last_mut().expect("open frame"), item);

            let child = frames.last_mut().and_then(next_child);
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    converted = close(frame);
                }
            }
        }
    }
}
impl TryFrom<StackItem> for StackValue {
    type Error = VmError;

    fn try_from(value: StackItem) -> VmResult<Self> {
        /// A compound whose elements are being converted.
        enum Frame {
            /// An `Array` or `Struct` under construction; `structure` picks
            /// which one.
            List {
                structure: bool,
                source: std::vec::IntoIter<StackItem>,
                items: Vec<StackValue>,
            },
            /// A `Map` under construction; elements arrive as key, value, ...
            Map {
                source: std::vec::IntoIter<StackItem>,
                entries: Vec<(StackValue, StackValue)>,
                pending_key: Option<StackValue>,
            },
        }

        fn convert_leaf(value: &StackItem) -> VmResult<Option<StackValue>> {
            match value {
                StackItem::Null => Ok(Some(StackValue::Null)),
                StackItem::Boolean(value) => Ok(Some(StackValue::Boolean(*value))),
                StackItem::Integer(value) => Ok(Some(match value.to_i64() {
                    Some(value) => StackValue::Integer(value),
                    None => StackValue::BigInteger(value.to_signed_bytes_le()),
                })),
                StackItem::ByteString(bytes) => Ok(Some(StackValue::ByteString(bytes.clone()))),
                StackItem::Buffer(buffer) => Ok(Some(StackValue::Buffer(buffer.data()))),
                StackItem::Pointer(pointer) => {
                    let position = i64::try_from(pointer.position()).map_err(|_| {
                        VmError::overflow("StackItem pointer position does not fit neo-vm-rs i64")
                    })?;
                    Ok(Some(StackValue::Pointer(position)))
                }
                StackItem::InteropInterface(_) => Err(VmError::invalid_operation_msg(
                    "Cannot convert InteropInterface into neo-vm-rs StackValue without a host handle",
                )),
                _ => Ok(None),
            }
        }

        fn open(value: StackItem) -> Frame {
            match value {
                StackItem::Array(array) => {
                    let items = array.items();
                    Frame::List {
                        structure: false,
                        items: Vec::with_capacity(items.len()),
                        source: items.into_iter(),
                    }
                }
                StackItem::Struct(structure) => {
                    let items = structure.items();
                    Frame::List {
                        structure: true,
                        items: Vec::with_capacity(items.len()),
                        source: items.into_iter(),
                    }
                }
                StackItem::Map(map) => {
                    let entries = map.iter();
                    let capacity = entries.len();
                    let mut source = Vec::with_capacity(capacity * 2);
                    for (key, entry) in entries {
                        source.push(key);
                        source.push(entry);
                    }
                    Frame::Map {
                        source: source.into_iter(),
                        entries: Vec::with_capacity(capacity),
                        pending_key: None,
                    }
                }
                _ => unreachable!("only compound items are opened"),
            }
        }

        fn next_child(frame: &mut Frame) -> Option<StackItem> {
            match frame {
                Frame::List { source, .. } | Frame::Map { source, .. } => source.next(),
            }
        }

        fn accept(frame: &mut Frame, item: StackValue) {
            match frame {
                Frame::List { items, .. } => items.push(item),
                Frame::Map {
                    entries,
                    pending_key,
                    ..
                } => match pending_key.take() {
                    None => *pending_key = Some(item),
                    Some(key) => entries.push((key, item)),
                },
            }
        }

        fn close(frame: Frame) -> StackValue {
            match frame {
                Frame::List {
                    structure, items, ..
                } => {
                    if structure {
                        StackValue::Struct(items)
                    } else {
                        StackValue::Array(items)
                    }
                }
                Frame::Map { entries, .. } => StackValue::Map(entries),
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(value);
        let mut converted = StackValue::Null;

        loop {
            if let Some(value) = pending.take() {
                if let Some(item) = convert_leaf(&value)? {
                    converted = item;
                } else {
                    let mut frame = open(value);
                    let first = next_child(&mut frame);
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            converted = close(frame);
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return Ok(converted);
            }
            let item = std::mem::replace(&mut converted, StackValue::Null);
            accept(frames.last_mut().expect("open frame"), item);

            let child = frames.last_mut().and_then(next_child);
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    converted = close(frame);
                }
            }
        }
    }
}
