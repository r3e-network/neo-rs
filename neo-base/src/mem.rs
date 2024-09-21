// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

pub trait ToReferences<R: ?Sized> {
    fn to_references(&self) -> Vec<&R>;
}

impl<R: ?Sized, T: AsRef<R>> ToReferences<R> for [T] {
    #[inline]
    fn to_references(&self) -> Vec<&R> { self.iter().map(|f| f.as_ref()).collect() }
}

#[cfg(test)]
mod test {
    use alloc::{string::ToString, vec};

    use super::*;

    #[test]
    fn test_to_references() {
        let s = vec!["a".to_string(), "b".to_string()];
        let f = |f: &str| {
            assert_eq!(f.len(), 1);
        };

        let r = s.to_references();
        f(r[0]);
        f(r[1]);
        assert_eq!(r.len(), 2);
    }
}
