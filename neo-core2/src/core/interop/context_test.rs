use std::collections::HashMap;
use crate::config;
use crate::core::block;
use crate::core::interop::Context;
use assert_matches::assert_matches;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hardfork_enabled() {
        // not configured
        {
            let ic = Context {
                hardforks: HashMap::from([
                    (config::HFAspidochelone.to_string(), 0),
                    (config::HFBasilisk.to_string(), 0),
                ]),
                block: block::Block {
                    header: block::Header { index: 10 },
                },
            };
            assert!(ic.is_hardfork_enabled(&config::HFAspidochelone));
            assert!(ic.is_hardfork_enabled(&config::HFBasilisk));
        }

        // new disabled
        {
            let ic = Context {
                hardforks: HashMap::from([(config::HFAspidochelone.to_string(), 5)]),
                block: block::Block {
                    header: block::Header { index: 10 },
                },
            };
            assert!(ic.is_hardfork_enabled(&config::HFAspidochelone));
            assert!(!ic.is_hardfork_enabled(&config::HFBasilisk));
        }

        // old enabled
        {
            let ic = Context {
                hardforks: HashMap::from([
                    (config::HFAspidochelone.to_string(), 0),
                    (config::HFBasilisk.to_string(), 10),
                ]),
                block: block::Block {
                    header: block::Header { index: 5 },
                },
            };
            assert!(ic.is_hardfork_enabled(&config::HFAspidochelone));
            assert!(!ic.is_hardfork_enabled(&config::HFBasilisk));
        }

        // not yet enabled
        {
            let ic = Context {
                hardforks: HashMap::from([(config::HFAspidochelone.to_string(), 10)]),
                block: block::Block {
                    header: block::Header { index: 5 },
                },
            };
            assert!(!ic.is_hardfork_enabled(&config::HFAspidochelone));
        }

        // already enabled
        {
            let ic = Context {
                hardforks: HashMap::from([(config::HFAspidochelone.to_string(), 10)]),
                block: block::Block {
                    header: block::Header { index: 15 },
                },
            };
            assert!(ic.is_hardfork_enabled(&config::HFAspidochelone));
        }
    }
}
