use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Type(u8);

impl Type {
    /// OnPersist is a trigger type that indicates that the script is being invoked
    /// internally by the system during block persistence (before transaction
    /// processing).
    pub const ON_PERSIST: Type = Type(0x01);

    /// PostPersist is a trigger type that indicates that the script is being invoked
    /// by the system after block persistence (transcation processing) has
    /// finished.
    pub const POST_PERSIST: Type = Type(0x02);

    /// The verification trigger indicates that the contract is being invoked as a verification function.
    /// The verification function can accept multiple parameters and should return a boolean value that indicates the validity of the transaction or block.
    /// The entry point of the contract will be invoked if the contract is triggered by Verification:
    ///     main(...);
    /// The entry point of the contract must be able to handle this type of invocation.
    pub const VERIFICATION: Type = Type(0x20);

    /// The application trigger indicates that the contract is being invoked as an application function.
    /// The application function can accept multiple parameters, change the states of the blockchain, and return any type of value.
    /// The contract can have any form of entry point, but we recommend that all contracts should have the following entry point:
    ///     public byte[] main(string operation, params object[] args)
    /// The functions can be invoked by creating an InvocationTransaction.
    pub const APPLICATION: Type = Type(0x40);

    /// All represents any trigger type.
    pub const ALL: Type = Type(Self::ON_PERSIST.0 | Self::POST_PERSIST.0 | Self::VERIFICATION.0 | Self::APPLICATION.0);
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ON_PERSIST => write!(f, "OnPersist"),
            Self::POST_PERSIST => write!(f, "PostPersist"),
            Self::VERIFICATION => write!(f, "Verification"),
            Self::APPLICATION => write!(f, "Application"),
            Self::ALL => write!(f, "All"),
            _ => write!(f, "Unknown"),
        }
    }
}

impl FromStr for Type {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "onpersist" => Ok(Self::ON_PERSIST),
            "postpersist" => Ok(Self::POST_PERSIST),
            "verification" => Ok(Self::VERIFICATION),
            "application" => Ok(Self::APPLICATION),
            "all" => Ok(Self::ALL),
            _ => Err(format!("unknown trigger type: {}", s)),
        }
    }
}
