// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum TriggerType {
    /// Triggered by system
    OnPersist = 0x01,

    /// Triggered by system
    PostPersist = 0x02,

    /// OnPersist | PostPersist
    System = 0x03,

    /// Triggered by Verifiable
    Verification = 0x20,

    /// Triggered by Transaction
    Application = 0x40,

    /// All of above
    All = 0x63,
}