
use std::hash::{Hash, Hasher};
use std::cmp::Ordering;

pub struct ByteArrayEqualityComparer;

impl ByteArrayEqualityComparer {
    pub const DEFAULT: Self = ByteArrayEqualityComparer;

    pub fn equals(&self, x: Option<&[u8]>, y: Option<&[u8]>) -> bool {
        match (x, y) {
            (Some(x), Some(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                if x.is_empty() {
                    return true;
                }
                unsafe {
                    let x_ptr = x.as_ptr();
                    let y_ptr = y.as_ptr();
                    let mut len = x.len();

                    while len >= 8 {
                        if *(x_ptr as *const u64) != *(y_ptr as *const u64) {
                            return false;
                        }
                        len -= 8;
                    }

                    let x_ptr = x_ptr.add(x.len() - len);
                    let y_ptr = y_ptr.add(y.len() - len);

                    for i in 0..len {
                        if *x_ptr.add(i) != *y_ptr.add(i) {
                            return false;
                        }
                    }
                }
                true
            }
            (None, None) => true,
            _ => false,
        }
    }

    pub fn hash<H: Hasher>(&self, obj: &[u8], state: &mut H) {
        let mut hash: u32 = 17;
        for &element in obj {
            hash = hash.wrapping_mul(31).wrapping_add(element as u32);
        }
        state.write_u32(hash);
    }
}

impl PartialEq for ByteArrayEqualityComparer {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ByteArrayEqualityComparer {}

impl Hash for ByteArrayEqualityComparer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(0);
    }
}

impl PartialOrd for ByteArrayEqualityComparer {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

impl Ord for ByteArrayEqualityComparer {
    fn cmp(&self, _other: &Self) -> Ordering {
        Ordering::Equal
    }
}
