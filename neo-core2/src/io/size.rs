use std::io::{self, Write};
use std::any::Any;
use std::fmt;
use std::mem::size_of;

// This structure is used to calculate the wire size of the serializable
// structure. It's an io::Write that doesn't do any real writes, but instead
// just counts the number of bytes to be written.
struct CounterWriter {
    counter: usize,
}

impl Write for CounterWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len();
        self.counter += n;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// get_var_int_size returns the size in number of bytes of a variable integer.
fn get_var_int_size(value: u64) -> usize {
    if value < 0xFD {
        1 // u8
    } else if value <= 0xFFFF {
        3 // byte + u16
    } else {
        5 // byte + u32
    }
}

// GetVarSize returns the number of bytes in a serialized variable. It supports ints/uints (estimating
// them with variable-length encoding that is used in Neo), strings, pointers to Serializable structures,
// slices and arrays of ints/uints or Serializable structures. It's similar to GetVarSize<T>(this T[] value)
// used in C#, but differs in that it also supports things like Uint160 or Uint256.
pub fn get_var_size(value: &dyn Any) -> usize {
    if let Some(s) = value.downcast_ref::<String>() {
        let value_size = s.len();
        return get_var_int_size(value_size as u64) + value_size;
    } else if let Some(i) = value.downcast_ref::<i64>() {
        return get_var_int_size(*i as u64);
    } else if let Some(u) = value.downcast_ref::<u64>() {
        return get_var_int_size(*u);
    } else if let Some(vser) = value.downcast_ref::<&dyn Serializable>() {
        let mut cw = CounterWriter { counter: 0 };
        let mut w = BinWriter::new(&mut cw);
        vser.encode_binary(&mut w);
        if let Some(err) = w.err {
            panic!("error serializing {}: {}", std::any::type_name::<&dyn Serializable>(), err);
        }
        return cw.counter;
    } else if let Some(slice) = value.downcast_ref::<Vec<Box<dyn Any>>>() {
        let value_length = slice.len();
        let mut value_size = 0;

        if value_length != 0 {
            match slice[0].as_ref().type_id() {
                id if id == std::any::TypeId::of::<Box<dyn Serializable>>() => {
                    for item in slice {
                        value_size += get_var_size(item.as_ref());
                    }
                }
                id if id == std::any::TypeId::of::<u8>() || id == std::any::TypeId::of::<i8>() => {
                    value_size = value_length;
                }
                id if id == std::any::TypeId::of::<u16>() || id == std::any::TypeId::of::<i16>() => {
                    value_size = value_length * 2;
                }
                id if id == std::any::TypeId::of::<u32>() || id == std::any::TypeId::of::<i32>() => {
                    value_size = value_length * 4;
                }
                id if id == std::any::TypeId::of::<u64>() || id == std::any::TypeId::of::<i64>() => {
                    value_size = value_length * 8;
                }
                _ => panic!("unsupported type in slice"),
            }
        }

        return get_var_int_size(value_length as u64) + value_size;
    } else {
        panic!("unable to calculate GetVarSize, {}", std::any::type_name::<&dyn Any>());
    }
}
