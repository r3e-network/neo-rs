mod interop {
    use crate::neogointernal;

    pub const HASH160_LEN: usize = 20;
    pub const HASH256_LEN: usize = 32;
    pub const PUBLIC_KEY_COMPRESSED_LEN: usize = 33;
    pub const PUBLIC_KEY_UNCOMPRESSED_LEN: usize = 65;
    pub const SIGNATURE_LEN: usize = 64;

    pub type Signature = Vec<u8>;
    pub type Hash160 = Vec<u8>;
    pub type Hash256 = Vec<u8>;
    pub type PublicKey = Vec<u8>;
    pub type Interface = Box<dyn std::any::Any>;

    impl Hash160 {
        pub fn equals(&self, b: &dyn std::any::Any) -> bool {
            bytes_equals(self, b)
        }
    }

    impl Hash256 {
        pub fn equals(&self, b: &dyn std::any::Any) -> bool {
            bytes_equals(self, b)
        }
    }

    impl PublicKey {
        pub fn equals(&self, b: &dyn std::any::Any) -> bool {
            bytes_equals(self, b)
        }
    }

    impl Signature {
        pub fn equals(&self, b: &dyn std::any::Any) -> bool {
            bytes_equals(self, b)
        }
    }

    fn bytes_equals(a: &dyn std::any::Any, b: &dyn std::any::Any) -> bool {
        if a.is::<String>() && b.is::<String>() {
            let a_str = a.downcast_ref::<String>().unwrap();
            let b_str = b.downcast_ref::<String>().unwrap();
            neogointernal::opcode2("EQUAL", a_str, b_str).unwrap_or(false)
        } else {
            false
        }
    }
}
