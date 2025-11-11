pub fn syscall_name_from_hash(hash: u32) -> Option<&'static str> {
    match hash {
        0x9647E7CF => Some("System.Runtime.Log"),
        0x616F0195 => Some("System.Runtime.Notify"),
        0xF6FC79B2 => Some("System.Runtime.Platform"),
        0x83FAB238 => Some("System.Runtime.Trigger"),
        0x43112784 => Some("System.Runtime.GetInvocationCounter"),
        0x8CEC27F8 => Some("System.Runtime.CheckWitness"),
        0x2789E347 => Some("System.Runtime.Time"),
        0xD0EC19F8 => Some("System.Runtime.ScriptHash"),
        0x616F90F1 => Some("System.Runtime.CallingScriptHash"),
        0xE0E75D31 => Some("System.Runtime.EntryScriptHash"),
        0x3C8722B7 => Some("System.Runtime.Script"),
        0x31E85D92 => Some("System.Storage.Get"),
        0x84183FE6 => Some("System.Storage.Put"),
        0xEDC5582F => Some("System.Storage.Delete"),
        0xCE67F69B => Some("System.Storage.GetContext"),
        _ => None,
    }
}
