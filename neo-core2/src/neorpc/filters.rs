use std::error::Error;
use std::fmt;

use crate::core::interop::runtime;
use crate::core::mempoolevent;
use crate::util;
use crate::vm::vmstate;

#[derive(Clone, Debug)]
pub struct BlockFilter {
    pub primary: Option<u8>,
    pub since: Option<u32>,
    pub till: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct TxFilter {
    pub sender: Option<util::Uint160>,
    pub signer: Option<util::Uint160>,
}

#[derive(Clone, Debug)]
pub struct NotificationFilter {
    pub contract: Option<util::Uint160>,
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExecutionFilter {
    pub state: Option<String>,
    pub container: Option<util::Uint256>,
}

#[derive(Clone, Debug)]
pub struct NotaryRequestFilter {
    pub sender: Option<util::Uint160>,
    pub signer: Option<util::Uint160>,
    pub type_: Option<mempoolevent::Type>,
}

pub trait SubscriptionFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Clone)]
pub struct InvalidSubscriptionFilter;

impl fmt::Display for InvalidSubscriptionFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid subscription filter")
    }
}

impl Error for InvalidSubscriptionFilter {}

impl BlockFilter {
    pub fn copy(&self) -> BlockFilter {
        BlockFilter {
            primary: self.primary,
            since: self.since,
            till: self.till,
        }
    }
}

impl SubscriptionFilter for BlockFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

impl TxFilter {
    pub fn copy(&self) -> TxFilter {
        TxFilter {
            sender: self.sender.clone(),
            signer: self.signer.clone(),
        }
    }
}

impl SubscriptionFilter for TxFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

impl NotificationFilter {
    pub fn copy(&self) -> NotificationFilter {
        NotificationFilter {
            contract: self.contract.clone(),
            name: self.name.clone(),
        }
    }
}

impl SubscriptionFilter for NotificationFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        if let Some(name) = &self.name {
            if name.len() > runtime::MAX_EVENT_NAME_LEN {
                return Err(Box::new(fmt::Error::new(
                    fmt::ErrorKind::InvalidInput,
                    format!(
                        "NotificationFilter name parameter must be less than {}",
                        runtime::MAX_EVENT_NAME_LEN
                    ),
                )));
            }
        }
        Ok(())
    }
}

impl ExecutionFilter {
    pub fn copy(&self) -> ExecutionFilter {
        ExecutionFilter {
            state: self.state.clone(),
            container: self.container.clone(),
        }
    }
}

impl SubscriptionFilter for ExecutionFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        if let Some(state) = &self.state {
            if state != &vmstate::HALT.to_string() && state != &vmstate::FAULT.to_string() {
                return Err(Box::new(fmt::Error::new(
                    fmt::ErrorKind::InvalidInput,
                    format!(
                        "ExecutionFilter state parameter must be either {} or {}",
                        vmstate::HALT, vmstate::FAULT
                    ),
                )));
            }
        }
        Ok(())
    }
}

impl NotaryRequestFilter {
    pub fn copy(&self) -> NotaryRequestFilter {
        NotaryRequestFilter {
            sender: self.sender.clone(),
            signer: self.signer.clone(),
            type_: self.type_.clone(),
        }
    }
}

impl SubscriptionFilter for NotaryRequestFilter {
    fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
