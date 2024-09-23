/// Module iterator provides functions to work with Neo iterators.
pub mod iterator {
    use crate::interop::neogointernal;

    /// Iterator represents a Neo iterator, it's an opaque data structure that can
    /// be properly created by storage.Find. Iterators range over key-value pairs,
    /// so it's convenient to use them for maps. This structure is similar in
    /// function to Neo .net framework's Iterator.
    pub struct Iterator;

    /// Next advances the iterator returning true if it was successful (and you
    /// can use Value to get value for slices or key-value pair for maps) and false
    /// otherwise (and there are no more elements in this Iterator). This function
    /// uses `System.Iterator.Next` syscall.
    pub fn next(it: &Iterator) -> bool {
        neogointernal::syscall1("System.Iterator.Next", it).unwrap()
    }

    /// Value returns iterator's current value. It's only valid to call after
    /// a successful Next call. This function uses `System.Iterator.Value` syscall.
    /// For slices, the result is just value.
    /// For maps, the result can be cast to a slice of 2 elements: a key and a value.
    /// For storage iterators, refer to `storage.FindFlags` documentation.
    pub fn value(it: &Iterator) -> neogointernal::Any {
        neogointernal::syscall1("System.Iterator.Value", it)
    }
}
