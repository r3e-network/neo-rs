use std::fmt::Write;

pub trait ByteExtensions {
    fn to_hex_string(&self) -> String;
    fn to_hex_string_with_reverse(&self, reverse: bool) -> String;
}

impl ByteExtensions for [u8] {
    fn to_hex_string(&self) -> String {
        let mut s = String::with_capacity(self.len() * 2);
        for &b in self {
            write!(&mut s, "{:02x}", b).unwrap();
        }
        s
    }

    fn to_hex_string_with_reverse(&self, reverse: bool) -> String {
        let mut s = String::with_capacity(self.len() * 2);
        if reverse {
            for &b in self.iter().rev() {
                write!(&mut s, "{:02x}", b).unwrap();
            }
        } else {
            for &b in self {
                write!(&mut s, "{:02x}", b).unwrap();
            }
        }
        s
    }
}

impl ByteExtensions for &[u8] {
    fn to_hex_string(&self) -> String {
        let mut s = String::with_capacity(self.len() * 2);
        for &b in *self {
            write!(&mut s, "{:02x}", b).unwrap();
        }
        s
    }

    fn to_hex_string_with_reverse(&self, reverse: bool) -> String {
        let mut s = String::with_capacity(self.len() * 2);
        if reverse {
            for &b in self.iter().rev() {
                write!(&mut s, "{:02x}", b).unwrap();
            }
        } else {
            for &b in *self {
                write!(&mut s, "{:02x}", b).unwrap();
            }
        }
        s
    }
}
