use crate::util::bitfield::BitField;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fields() {
        let mut a = BitField::new(128);
        let mut b = BitField::new(128);
        a.set(10);
        b.set(10);
        a.set(42);
        b.set(42);
        a.set(100);
        b.set(100);
        assert!(a.is_set(42));
        assert!(!b.is_set(43));
        assert!(a.is_subset(&b));

        let v = 1u64 << 10 | 1u64 << 42;
        assert_eq!(v, a[0]);
        assert_eq!(v, b[0]);

        assert!(a.equals(&b));

        let mut c = a.copy();
        assert!(c.equals(&b));

        let z = BitField::new(128);
        assert!(z.is_subset(&c));
        c.and(&a);
        assert!(c.equals(&b));
        c.and(&z);
        assert!(c.equals(&z));

        c = BitField::new(64);
        assert!(!z.is_subset(&c));
        c[0] = a[0];
        assert!(!c.equals(&a));
        assert!(c.is_subset(&a));

        b.and(&c);
        assert!(!b.equals(&a));
    }
}
