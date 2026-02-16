// Copyright (C) 2015-2025 The Neo Project.
//
// security_fixes.rs - Security utilities and patches for native contracts
//
//! Security utilities for native contracts
//!
//! This module provides security enhancements including:
//! - Overflow protection for arithmetic operations
//! - Reentrancy guards for state-changing operations
//! - Permission validation helpers
//! - State consistency checks

use crate::error::{CoreError, CoreResult};
use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    /// Track reentrancy guards for native contract operations
    static REENTRANCY_GUARDS: RefCell<HashSet<ReentrancyGuardType>> = RefCell::new(HashSet::new());
}

/// Types of operations that need reentrancy protection
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReentrancyGuardType {
    /// GAS token transfer
    GasTransfer,
    /// GAS token mint
    GasMint,
    /// GAS token burn
    GasBurn,
    /// NEO token transfer
    NeoTransfer,
    /// NEO token vote
    NeoVote,
    /// Contract deploy
    ContractDeploy,
    /// Contract update
    ContractUpdate,
    /// Contract destroy
    ContractDestroy,
    /// Policy setting update
    PolicyUpdate,
}

/// Security context for native contract operations
pub struct SecurityContext;

impl SecurityContext {
    /// Check if a reentrancy guard is currently active
    pub fn is_guarded(guard_type: ReentrancyGuardType) -> bool {
        REENTRANCY_GUARDS.with(|guards| guards.borrow().contains(&guard_type))
    }

    /// Enter a guarded section
    pub fn enter_guard(guard_type: ReentrancyGuardType) -> CoreResult<Guard> {
        REENTRANCY_GUARDS.with(|guards| {
            let mut guards = guards.borrow_mut();
            if guards.contains(&guard_type) {
                return Err(CoreError::native_contract(format!(
                    "Reentrancy detected for operation {:?}",
                    guard_type
                )));
            }
            guards.insert(guard_type);
            Ok(Guard { guard_type })
        })
    }

    /// Exit a guarded section (called automatically when Guard is dropped)
    fn exit_guard(guard_type: ReentrancyGuardType) {
        REENTRANCY_GUARDS.with(|guards| {
            guards.borrow_mut().remove(&guard_type);
        });
    }

    /// Reset all guards (for testing/emergency)
    pub fn reset_all_guards() {
        REENTRANCY_GUARDS.with(|guards| {
            guards.borrow_mut().clear();
        });
    }
}

/// RAII guard for reentrancy protection
pub struct Guard {
    guard_type: ReentrancyGuardType,
}

impl Drop for Guard {
    fn drop(&mut self) {
        SecurityContext::exit_guard(self.guard_type);
    }
}

/// Safe arithmetic operations for native contracts
pub struct SafeArithmetic;

impl SafeArithmetic {
    /// Safely add two BigInt values, checking for overflow
    pub fn safe_add(a: &BigInt, b: &BigInt) -> CoreResult<BigInt> {
        // BigInt in Rust has arbitrary precision, so overflow isn't a concern
        // However, we check for negative results in contexts where they shouldn't occur
        Ok(a + b)
    }

    /// Safely subtract two BigInt values, checking the result doesn't go negative
    pub fn safe_sub(a: &BigInt, b: &BigInt) -> CoreResult<BigInt> {
        let result = a - b;
        if result.is_negative() {
            return Err(CoreError::native_contract(
                "Subtraction would result in negative value".to_string(),
            ));
        }
        Ok(result)
    }

    /// Safely multiply two BigInt values
    pub fn safe_mul(a: &BigInt, b: &BigInt) -> CoreResult<BigInt> {
        Ok(a * b)
    }

    /// Safely divide two BigInt values
    pub fn safe_div(a: &BigInt, b: &BigInt) -> CoreResult<BigInt> {
        if b.is_zero() {
            return Err(CoreError::native_contract("Division by zero"));
        }
        Ok(a / b)
    }

    /// Check if addition would overflow (for fixed-size integers)
    pub fn check_add_overflow<T>(a: T, b: T) -> CoreResult<T>
    where
        T: num_traits::CheckedAdd + Copy + std::fmt::Display,
    {
        a.checked_add(&b)
            .ok_or_else(|| CoreError::native_contract(format!("Addition overflow: {} + {}", a, b)))
    }

    /// Check if subtraction would underflow
    pub fn check_sub_underflow<T>(a: T, b: T) -> CoreResult<T>
    where
        T: num_traits::CheckedSub + Copy + std::fmt::Display,
    {
        a.checked_sub(&b).ok_or_else(|| {
            CoreError::native_contract(format!("Subtraction underflow: {} - {}", a, b))
        })
    }

    /// Check if multiplication would overflow
    pub fn check_mul_overflow<T>(a: T, b: T) -> CoreResult<T>
    where
        T: num_traits::CheckedMul + Copy + std::fmt::Display,
    {
        a.checked_mul(&b).ok_or_else(|| {
            CoreError::native_contract(format!("Multiplication overflow: {} * {}", a, b))
        })
    }

    /// Validate that a balance change is safe
    pub fn validate_balance_change(
        current: &BigInt,
        delta: &BigInt,
        allow_negative: bool,
    ) -> CoreResult<BigInt> {
        if delta.is_negative() && !allow_negative {
            let abs_delta = -delta;
            if current < &abs_delta {
                return Err(CoreError::native_contract(
                    "Insufficient balance for operation".to_string(),
                ));
            }
        }
        Ok(current + delta)
    }

    /// Validate that total supply after change is valid
    pub fn validate_total_supply_change(current: &BigInt, delta: &BigInt) -> CoreResult<BigInt> {
        let new_supply = current + delta;
        if new_supply.is_negative() {
            return Err(CoreError::native_contract(
                "Total supply cannot be negative".to_string(),
            ));
        }
        Ok(new_supply)
    }
}

/// Permission validation utilities
pub struct PermissionValidator;

impl PermissionValidator {
    /// Validate that a value is within a valid range
    pub fn validate_range<T>(value: T, min: T, max: T, name: &str) -> CoreResult<()>
    where
        T: PartialOrd + std::fmt::Display,
    {
        if value < min || value > max {
            return Err(CoreError::native_contract(format!(
                "{} must be between {} and {}, got {}",
                name, min, max, value
            )));
        }
        Ok(())
    }

    /// Validate that an amount is non-negative
    pub fn validate_non_negative(amount: &BigInt, name: &str) -> CoreResult<()> {
        if amount.is_negative() {
            return Err(CoreError::native_contract(format!(
                "{} cannot be negative",
                name
            )));
        }
        Ok(())
    }

    /// Validate that an amount is positive
    pub fn validate_positive(amount: &BigInt, name: &str) -> CoreResult<()> {
        if *amount <= BigInt::zero() {
            return Err(CoreError::native_contract(format!(
                "{} must be positive",
                name
            )));
        }
        Ok(())
    }

    /// Validate account hash format
    pub fn validate_account_hash(data: &[u8]) -> CoreResult<()> {
        if data.len() != 20 {
            return Err(CoreError::native_contract(format!(
                "Account hash must be 20 bytes, got {}",
                data.len()
            )));
        }
        Ok(())
    }

    /// Validate public key format
    pub fn validate_public_key(data: &[u8]) -> CoreResult<()> {
        if data.len() != 33 {
            return Err(CoreError::native_contract(format!(
                "Public key must be 33 bytes, got {}",
                data.len()
            )));
        }
        // Check first byte is valid (0x02 or 0x03 for compressed keys)
        if data[0] != 0x02 && data[0] != 0x03 {
            return Err(CoreError::native_contract(
                "Invalid public key format: first byte must be 0x02 or 0x03".to_string(),
            ));
        }
        Ok(())
    }
}

/// State consistency validation
pub struct StateValidator;

impl StateValidator {
    /// Validate that account state is consistent
    pub fn validate_account_state(
        balance: &BigInt,
        balance_height: u32,
        current_height: u32,
    ) -> CoreResult<()> {
        if balance.is_negative() {
            return Err(CoreError::native_contract(
                "Account balance cannot be negative".to_string(),
            ));
        }
        if balance_height > current_height {
            return Err(CoreError::native_contract(
                "Account balance height cannot be in the future".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate that candidate state is consistent
    pub fn validate_candidate_state(registered: bool, votes: &BigInt) -> CoreResult<()> {
        if votes.is_negative() {
            return Err(CoreError::native_contract(
                "Candidate votes cannot be negative".to_string(),
            ));
        }
        if !registered && !votes.is_zero() {
            return Err(CoreError::native_contract(
                "Unregistered candidate should have zero votes".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate total supply consistency
    pub fn validate_total_supply(
        total_supply: &BigInt,
        max_supply: Option<&BigInt>,
    ) -> CoreResult<()> {
        if total_supply.is_negative() {
            return Err(CoreError::native_contract(
                "Total supply cannot be negative".to_string(),
            ));
        }
        if let Some(max) = max_supply {
            if total_supply > max {
                return Err(CoreError::native_contract(format!(
                    "Total supply {} exceeds maximum {}",
                    total_supply, max
                )));
            }
        }
        Ok(())
    }

    /// Validate voters count consistency
    pub fn validate_voters_count(voters_count: &BigInt, total_votes: &BigInt) -> CoreResult<()> {
        if voters_count.is_negative() {
            return Err(CoreError::native_contract(
                "Voters count cannot be negative".to_string(),
            ));
        }
        if total_votes.is_negative() {
            return Err(CoreError::native_contract(
                "Total votes cannot be negative".to_string(),
            ));
        }
        // Voters count should not exceed total votes (each voter has at least 1 vote)
        if voters_count > total_votes {
            return Err(CoreError::native_contract(
                "Voters count exceeds total votes".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reentrancy_guard() {
        // Reset guards first
        SecurityContext::reset_all_guards();

        // Enter guard
        let guard = SecurityContext::enter_guard(ReentrancyGuardType::GasTransfer);
        assert!(guard.is_ok());

        // Try to enter again (should fail)
        let guard2 = SecurityContext::enter_guard(ReentrancyGuardType::GasTransfer);
        assert!(guard2.is_err());

        // Different guard type should work
        let guard3 = SecurityContext::enter_guard(ReentrancyGuardType::NeoTransfer);
        assert!(guard3.is_ok());

        // Clean up
        SecurityContext::reset_all_guards();
    }

    #[test]
    fn test_safe_sub() {
        let a = BigInt::from(100);
        let b = BigInt::from(50);
        let result = SafeArithmetic::safe_sub(&a, &b);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BigInt::from(50));

        // Should fail for negative result
        let c = BigInt::from(100);
        let result = SafeArithmetic::safe_sub(&b, &c);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_balance_change() {
        let current = BigInt::from(100);
        let delta = BigInt::from(-50);
        let result = SafeArithmetic::validate_balance_change(&current, &delta, false);
        assert!(result.is_ok());

        // Should fail if not allowing negative
        let delta = BigInt::from(-150);
        let result = SafeArithmetic::validate_balance_change(&current, &delta, false);
        assert!(result.is_err());

        // Should succeed if allowing negative
        let result = SafeArithmetic::validate_balance_change(&current, &delta, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_total_supply_change() {
        let current = BigInt::from(100);
        let delta = BigInt::from(-50);
        let result = SafeArithmetic::validate_total_supply_change(&current, &delta);
        assert!(result.is_ok());

        // Should fail if negative
        let delta = BigInt::from(-150);
        let result = SafeArithmetic::validate_total_supply_change(&current, &delta);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_range() {
        assert!(PermissionValidator::validate_range(50u32, 0, 100, "test").is_ok());
        assert!(PermissionValidator::validate_range(150u32, 0, 100, "test").is_err());
        assert!(PermissionValidator::validate_range(0u32, 1, 100, "test").is_err());
    }

    #[test]
    fn test_validate_account_hash() {
        assert!(PermissionValidator::validate_account_hash(&[0u8; 20]).is_ok());
        assert!(PermissionValidator::validate_account_hash(&[0u8; 19]).is_err());
        assert!(PermissionValidator::validate_account_hash(&[0u8; 21]).is_err());
    }

    #[test]
    fn test_validate_public_key() {
        let mut valid_key = vec![0x02u8];
        valid_key.extend_from_slice(&[0u8; 32]);
        assert!(PermissionValidator::validate_public_key(&valid_key).is_ok());

        let mut valid_key2 = vec![0x03u8];
        valid_key2.extend_from_slice(&[0u8; 32]);
        assert!(PermissionValidator::validate_public_key(&valid_key2).is_ok());

        // Invalid first byte
        let mut invalid_key = vec![0x04u8];
        invalid_key.extend_from_slice(&[0u8; 32]);
        assert!(PermissionValidator::validate_public_key(&invalid_key).is_err());

        // Wrong length
        assert!(PermissionValidator::validate_public_key(&[0x02u8; 32]).is_err());
    }

    #[test]
    fn test_state_validator() {
        // Test account state validation
        assert!(StateValidator::validate_account_state(&BigInt::from(100), 100, 200).is_ok());

        assert!(StateValidator::validate_account_state(&BigInt::from(-100), 100, 200).is_err());

        assert!(StateValidator::validate_account_state(&BigInt::from(100), 300, 200).is_err());

        // Test candidate state validation
        assert!(StateValidator::validate_candidate_state(true, &BigInt::from(100)).is_ok());
        assert!(StateValidator::validate_candidate_state(false, &BigInt::from(0)).is_ok());
        assert!(StateValidator::validate_candidate_state(false, &BigInt::from(100)).is_err());

        // Test total supply validation
        assert!(StateValidator::validate_total_supply(&BigInt::from(100), None).is_ok());
        assert!(
            StateValidator::validate_total_supply(&BigInt::from(100), Some(&BigInt::from(1000)))
                .is_ok()
        );
        assert!(StateValidator::validate_total_supply(&BigInt::from(-100), None).is_err());
        assert!(
            StateValidator::validate_total_supply(&BigInt::from(1000), Some(&BigInt::from(100)))
                .is_err()
        );
    }
}
