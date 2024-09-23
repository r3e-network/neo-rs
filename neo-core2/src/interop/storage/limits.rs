// Contract storage limits.

// MaxKeyLen is the maximum length of a key for storage items.
// Contracts can't use keys longer than that in their requests to the DB.
pub const MAX_KEY_LEN: usize = 64;

// MaxValueLen is the maximum length of a value for storage items.
// It is set to be the maximum value for uint16, contracts can't put
// values longer than that into the DB.
pub const MAX_VALUE_LEN: usize = 65535;
