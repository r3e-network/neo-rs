// AttributeType represents a transaction attribute type.
#[repr(u8)]
enum AttributeType {
    HighPriorityT = 1,
    OracleResponseT = 0x11,
    NotValidBeforeT = 0x20,
    ConflictsT = 0x21,
    // NotaryAssistedT is an extension of Neo protocol available on specifically configured NeoGo networks.
    NotaryAssistedT = 0x22,
}
