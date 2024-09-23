/*
Package storage provides functions to access and modify contract's storage.
Neo storage's model follows simple key-value DB pattern, this storage is a part
of blockchain state, so you can use it between various invocations of the same
contract.
*/

use crate::interop::iterator::Iterator;
use crate::interop::neogointernal;

// Context represents storage context that is mandatory for Put/Get/Delete
// operations. It's an opaque type that can only be created properly by
// GetContext, GetReadOnlyContext or ConvertContextToReadOnly. It's similar
// to Neo .net framework's StorageContext class.
pub struct Context;

// FindFlags represents parameters to `Find` iterator.
#[derive(Clone, Copy)]
pub enum FindFlags {
    None = 0,
    KeysOnly = 1 << 0,
    RemovePrefix = 1 << 1,
    ValuesOnly = 1 << 2,
    DeserializeValues = 1 << 3,
    PickField0 = 1 << 4,
    PickField1 = 1 << 5,
    Backwards = 1 << 7,
}

// ConvertContextToReadOnly returns new context from the given one, but with
// writing capability turned off, so that you could only invoke Get and Find
// using this new Context. If Context is already read-only this function is a
// no-op. It uses `System.Storage.AsReadOnly` syscall.
pub fn convert_context_to_read_only(ctx: Context) -> Context {
    neogointernal::syscall1("System.Storage.AsReadOnly", ctx).into()
}

// GetContext returns current contract's (that invokes this function) storage
// context. It uses `System.Storage.GetContext` syscall.
pub fn get_context() -> Context {
    neogointernal::syscall0("System.Storage.GetContext").into()
}

// GetReadOnlyContext returns current contract's (that invokes this function)
// storage context in read-only mode, you can use this context for Get and Find
// functions, but using it for Put and Delete will fail. It uses
// `System.Storage.GetReadOnlyContext` syscall.
pub fn get_read_only_context() -> Context {
    neogointernal::syscall0("System.Storage.GetReadOnlyContext").into()
}

// Put saves given value with given key in the storage using given Context.
// Even though it accepts interface{} hidden under `any` for both, you can only
// pass simple types there like string, []byte, int or bool (not structures or
// slices of more complex types). To put more complex types there serialize them
// first using runtime.Serialize. This function uses `System.Storage.Put` syscall.
pub fn put(ctx: Context, key: impl Into<neogointernal::Any>, value: impl Into<neogointernal::Any>) {
    neogointernal::syscall3_no_return("System.Storage.Put", ctx, key.into(), value.into())
}

// Get retrieves value stored for the given key using given Context. See Put
// documentation on possible key and value types. If the value is not present in
// the database it returns nil. This function uses `System.Storage.Get` syscall.
pub fn get(ctx: Context, key: impl Into<neogointernal::Any>) -> neogointernal::Any {
    neogointernal::syscall2("System.Storage.Get", ctx, key.into())
}

// Delete removes key-value pair from storage by the given key using given
// Context. See Put documentation on possible key types. This function uses
// `System.Storage.Delete` syscall.
pub fn delete(ctx: Context, key: impl Into<neogointernal::Any>) {
    neogointernal::syscall2_no_return("System.Storage.Delete", ctx, key.into())
}

// Find returns an iterator.Iterator over key-value pairs in the given Context
// that match the given key (contain it as a prefix). See Put documentation on
// possible key types and iterator package documentation on how to use the
// returned value. This function uses `System.Storage.Find` syscall.
pub fn find(ctx: Context, key: impl Into<neogointernal::Any>, options: FindFlags) -> Iterator {
    neogointernal::syscall3("System.Storage.Find", ctx, key.into(), options as u8).into()
}
