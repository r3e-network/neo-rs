use std::collections::HashMap;

// Contract represents a deployed contract.
#[derive(Debug, Clone)]
struct Contract {
    id: i32,
    update_counter: i32,
    hash: Hash160,
    nef: Vec<u8>,
    manifest: Manifest,
}

// ParameterType represents smartcontract parameter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterType {
    AnyType = 0x00,
    BoolType = 0x10,
    IntegerType = 0x11,
    ByteArrayType = 0x12,
    StringType = 0x13,
    Hash160Type = 0x14,
    Hash256Type = 0x15,
    PublicKeyType = 0x16,
    SignatureType = 0x17,
    ArrayType = 0x20,
    MapType = 0x22,
    InteropInterfaceType = 0x30,
    VoidType = 0xff,
}

// Manifest represents contract's manifest.
#[derive(Debug, Clone)]
struct Manifest {
    name: String,
    groups: Vec<Group>,
    features: HashMap<String, String>,
    supported_standards: Vec<String>,
    abi: ABI,
    permissions: Vec<Permission>,
    trusts: Vec<Hash160>,
    extra: Option<serde_json::Value>,
}

// ABI represents contract's ABI.
#[derive(Debug, Clone)]
struct ABI {
    methods: Vec<Method>,
    events: Vec<Event>,
}

// Method represents a contract method.
#[derive(Debug, Clone)]
struct Method {
    name: String,
    params: Vec<Parameter>,
    return_type: ParameterType,
    offset: i32,
    safe: bool,
}

// Event represents a contract event.
#[derive(Debug, Clone)]
struct Event {
    name: String,
    params: Vec<Parameter>,
}

// Parameter represents a method parameter.
#[derive(Debug, Clone)]
struct Parameter {
    name: String,
    param_type: ParameterType,
}

// Permission represents contract permission.
#[derive(Debug, Clone)]
struct Permission {
    contract: Hash160,
    methods: Vec<String>,
}

// Group represents a manifest group.
#[derive(Debug, Clone)]
struct Group {
    public_key: PublicKey,
    signature: Signature,
}

// Placeholder types for interop types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Hash160([u8; 20]);

#[derive(Debug, Clone)]
struct PublicKey(Vec<u8>);

#[derive(Debug, Clone)]
struct Signature(Vec<u8>);
