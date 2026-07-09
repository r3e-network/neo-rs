use std::collections::HashSet;
use std::sync::Arc;

use neo_vm_rs::ExecutionEngineLimits;

use crate::error::{VmError, VmResult};

use super::{CompoundIdentity, StackItem, compound_identity};

/// Implements the C# base `StackItem.Equals(other)` virtual dispatch used by the
/// `EQUAL`/`NOTEQUAL` opcodes (`JumpTable.Bitwisee.cs:89` → `x1.Equals(x2, limits)`).
///
/// This is the non-faulting comparison path. It mirrors each concrete type's
/// `Equals(StackItem)` override in `neo_csharp_vm/src/Neo.VM/Types`:
/// - `Integer`/`Boolean`: value equality within the same concrete type, else false
///   (TYPE-STRICT — `Integer(1) != ByteString([1])`, verified against mainnet C# v3.10.1).
/// - `ByteString`: byte equality within the same type (no budget here; the budgeted
///   variant is handled directly by [`StackItem::equals_with_limits`]).
/// - `Pointer`: position + originating-script equality (`Pointer.cs:46-51`).
/// - `Null`: `other is Null` (`Null.cs:38-42`).
/// - `Array`/`Map`/`Buffer`/`InteropInterface`: REFERENCE equality, because these
///   types do NOT override `Equals` and fall back to the base
///   `ReferenceEquals(this, other)` (`StackItem.cs:117-120`). NOTE: this differs from
///   the structural equality used by [`StackItem::equals`] (Rust `PartialEq`, used for
///   map keys / reference counting), which must stay structural.
fn equals_plain(a: &StackItem, b: &StackItem) -> bool {
    match (a, b) {
        (StackItem::Null, StackItem::Null) => true,
        (StackItem::Boolean(x), StackItem::Boolean(y)) => x == y,
        (StackItem::Integer(x), StackItem::Integer(y)) => x.to_bigint() == y.to_bigint(),
        (StackItem::ByteString(x), StackItem::ByteString(y)) => x == y,
        (StackItem::Pointer(x), StackItem::Pointer(y)) => x == y,
        (StackItem::InteropInterface(x), StackItem::InteropInterface(y)) => Arc::ptr_eq(x, y),
        (StackItem::Buffer(x), StackItem::Buffer(y)) => x.id() == y.id(),
        // Array/Map/Struct fall back to the base `ReferenceEquals` (identity).
        (StackItem::Array(_), _) | (StackItem::Struct(_), _) | (StackItem::Map(_), _) => {
            match (compound_identity(a), compound_identity(b)) {
                (Some(ia), Some(ib)) => ia == ib,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Implements C# `ByteString.Equals(other, ref limits)` (`ByteString.cs:60-78`).
///
/// Faults (returns `Err`) when `self`'s size exceeds the remaining budget or the
/// budget is already exhausted, and decrements the budget by the compared size.
fn byte_string_size_eq_with_budget(
    a: &[u8],
    other: &StackItem,
    limits: &mut u32,
) -> VmResult<bool> {
    let a_size = a.len() as u64;
    if a_size > u64::from(*limits) || *limits == 0 {
        return Err(VmError::invalid_operation_msg(format!(
            "The operand exceeds the maximum comparable size, {a_size}/{limits}."
        )));
    }
    // comparedSize starts at 1 (C# `uint comparedSize = 1;`).
    let mut compared_size: u64 = 1;
    let result = match other {
        StackItem::ByteString(b) => {
            compared_size = compared_size.max(a_size).max(b.len() as u64);
            if (b.len() as u64) > u64::from(*limits) {
                // Decrement still runs in C#'s `finally` before the throw propagates,
                // but the throw fails the engine regardless, so surface the fault here.
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{limits}.",
                    b.len()
                )));
            }
            Ok(a == b.as_slice())
        }
        // C# `other is not ByteString b` → return false (comparedSize stays 1).
        _ => Ok(false),
    };
    // C# `finally { limits -= comparedSize; }` — compared_size is bounded by *limits
    // on every non-faulting path, so the subtraction cannot underflow.
    *limits = limits.saturating_sub(compared_size as u32);
    result
}

impl StackItem {
    /// Checks if two stack items are equal.
    pub fn equals(&self, other: &Self) -> VmResult<bool> {
        self.equals_with_refs(other, &mut HashSet::new())
    }

    /// Checks if two stack items are equal under the `EQUAL`/`NOTEQUAL` opcode rules.
    ///
    /// Faithful port of C# `StackItem.Equals(other, ExecutionEngineLimits)` dispatch,
    /// invoked from `JumpTable.Bitwisee.cs:89` (`x1.Equals(x2, engine.Limits)`). The
    /// comparison is dispatched on `self`'s concrete type:
    /// - `ByteString` → `ByteString.cs:54-78`: size/budget-checked byte equality
    ///   (FAULTS when either operand exceeds `MaxComparableSize`).
    /// - `Struct` → `Struct.cs:91-132`: iterative two-stack structural walk bounded by
    ///   `MaxStackSize` (item count) and `MaxComparableSize` (comparable budget); FAULTS
    ///   on overflow of either budget.
    /// - everything else → base `StackItem.Equals(other)` (`equals_plain`): value
    ///   equality for `Integer`/`Boolean`/`ByteString`/`Pointer`, `other is Null` for
    ///   `Null`, and REFERENCE equality for `Array`/`Map`/`Buffer`/`InteropInterface`.
    ///
    /// This intentionally differs from [`StackItem::equals`] (the structural `PartialEq`
    /// used for map keys and reference counting): under `EQUAL`, `Array`/`Map` use
    /// REFERENCE semantics, matching the C# reference VM.
    pub fn equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        match self {
            Self::ByteString(bytes) => {
                let mut budget = limits.max_comparable_size;
                byte_string_size_eq_with_budget(bytes, other, &mut budget)
            }
            Self::Struct(_) => self.struct_equals_with_limits(other, limits),
            _ => Ok(equals_plain(self, other)),
        }
    }

    /// Port of C# `Struct.Equals(other, ExecutionEngineLimits)` (`Struct.cs:91-132`).
    ///
    /// Iterative two-stack walk that bounds the number of compared items by
    /// `MaxStackSize` and the cumulative comparable size by `MaxComparableSize`,
    /// faulting (`Err`) when either budget is exceeded.
    fn struct_equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        // `other is not Struct s => return false`
        let other_struct = match other {
            Self::Struct(s) => s,
            _ => return Ok(false),
        };
        let self_struct = match self {
            Self::Struct(s) => s,
            // Unreachable: only called with `self` being a Struct.
            _ => return Ok(false),
        };

        let mut stack1: Vec<StackItem> = vec![Self::Struct(self_struct.clone())];
        let mut stack2: Vec<StackItem> = vec![Self::Struct(other_struct.clone())];
        let mut count = limits.max_stack_size;
        let mut max_comparable_size = limits.max_comparable_size;

        while let Some(a) = stack1.pop() {
            // C# `if (count-- == 0) throw` — fault once the item budget is exhausted.
            if count == 0 {
                return Err(VmError::invalid_operation_msg(
                    "Too many struct items to compare in struct comparison.",
                ));
            }
            count -= 1;

            let b = stack2.pop().ok_or_else(|| {
                VmError::invalid_operation_msg("Struct comparison stack underflow")
            })?;

            if let Self::ByteString(bytes) = &a {
                if !byte_string_size_eq_with_budget(bytes, &b, &mut max_comparable_size)? {
                    return Ok(false);
                }
            } else {
                // C# `if (maxComparableSize == 0) throw; maxComparableSize -= 1;`
                if max_comparable_size == 0 {
                    return Err(VmError::invalid_operation_msg(
                        "The operand exceeds the maximum comparable size in struct comparison.",
                    ));
                }
                max_comparable_size -= 1;

                if let Self::Struct(sa) = &a {
                    // `if (ReferenceEquals(a, b)) continue;`
                    if let Self::Struct(sb) = &b {
                        if sa.id() == sb.id() {
                            continue;
                        }
                        // `if (sa.Count != sb.Count) return false;`
                        if sa.len() != sb.len() {
                            return Ok(false);
                        }
                        for item in sa.iter() {
                            stack1.push(item);
                        }
                        for item in sb.iter() {
                            stack2.push(item);
                        }
                    } else {
                        // `if (b is not Struct sb) return false;`
                        return Ok(false);
                    }
                } else {
                    // C# base virtual `a.Equals(b)` (reference for compounds, value otherwise).
                    if !equals_plain(&a, &b) {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    /// Checks if two stack items are equal with reference tracking to handle cycles.
    fn equals_with_refs(
        &self,
        other: &Self,
        visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
    ) -> VmResult<bool> {
        let mut visited_key = None;
        if let (Some(self_id), Some(other_id)) = (compound_identity(self), compound_identity(other))
        {
            if visited.contains(&(self_id, other_id)) || visited.contains(&(other_id, self_id)) {
                return Ok(true);
            }

            visited.insert((self_id, other_id));
            visited_key = Some((self_id, other_id));
        }

        // C# Neo VM PrimitiveType.Equals is TYPE-STRICT: only items of the SAME
        // concrete primitive type (Integer, ByteString, Boolean) compare equal,
        // and only by value within that type. Cross-type comparison (e.g.
        // `Integer(1) == ByteString([0x01])`) returns FALSE in C#, even when
        // the byte representations match. Verified via mainnet RPC invokescript
        // against C# v3.10.1.
        let result = match (self, other) {
            (Self::Null, Self::Null) => Ok(true),
            // Buffer uses reference equality (compound type in C# Neo VM).
            // Buffer == Buffer → same reference only; Buffer == anything_else → false.
            (Self::Buffer(a), Self::Buffer(b)) => Ok(a.id() == b.id()),
            (Self::Buffer(_), _) | (_, Self::Buffer(_)) => Ok(false),
            // Same-type primitive comparisons (TYPE-STRICT, matches C# PrimitiveType.Equals).
            (Self::Boolean(a), Self::Boolean(b)) => Ok(a == b),
            (Self::Integer(a), Self::Integer(b)) => Ok(a.to_bigint() == b.to_bigint()),
            (Self::ByteString(a), Self::ByteString(b)) => Ok(a == b),
            // Cross-type primitive comparison: always FALSE (no byte-wise coercion).
            (a, b)
                if matches!(a, Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_))
                    && matches!(b, Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_)) =>
            {
                Ok(false)
            }
            (Self::Pointer(a), Self::Pointer(b)) => Ok(a == b),
            (Self::InteropInterface(a), Self::InteropInterface(b)) => Ok(Arc::ptr_eq(a, b)),
            (Self::Array(a), Self::Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                for (ai, bi) in a.iter().zip(b.iter()) {
                    if !ai.equals_with_refs(&bi, visited)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            (Self::Struct(a), Self::Struct(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                for (ai, bi) in a.iter().zip(b.iter()) {
                    if !ai.equals_with_refs(&bi, visited)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            (Self::Map(a), Self::Map(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                let b_items = b.items();
                for (ak, av) in a.items().iter() {
                    let found = b_items.iter().any(|(bk, bv)| {
                        ak.equals_with_refs(bk, visited).unwrap_or(false)
                            && av.equals_with_refs(bv, visited).unwrap_or(false)
                    });

                    if !found {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            _ => Ok(false),
        };

        if let Some((self_id, other_id)) = visited_key {
            visited.remove(&(self_id, other_id));
        }

        result
    }
}
