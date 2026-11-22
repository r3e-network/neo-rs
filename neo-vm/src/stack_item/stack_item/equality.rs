use super::*;

impl StackItem {
    /// Checks if two stack items are equal.
    pub fn equals(&self, other: &Self) -> VmResult<bool> {
        self.equals_with_refs(other, &mut std::collections::HashSet::new())
    }

    /// Checks if two stack items are equal with execution limits (aligns with C# API).
    ///
    /// C# equality is deliberately asymmetric across item kinds:
    /// - primitives compare by value (type-strict);
    /// - `Buffer`, `Array` and `Map` compare by reference;
    /// - `Struct` compares structurally, bounded by `MaxStackSize` and
    ///   `MaxComparableSize`;
    /// - `ByteString` comparisons consume `MaxComparableSize`.
    ///
    /// The previous implementation reused the un-bounded structural `equals`
    /// here, which made `EQUAL` on arrays/maps consensus-incompatible and
    /// allowed unbounded recursive comparisons.
    pub fn equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        let mut budget = LimitBudget {
            remaining_stack: u64::from(limits.max_stack_size),
            remaining_comparable: i64::from(limits.max_comparable_size),
            visited: std::collections::HashSet::new(),
        };
        self.equals_with_budget(other, &mut budget)
    }

    /// Walks the pair on an explicit step stack.
    ///
    /// C# `Struct.Equals(StackItem other, ExecutionLimits limits)` keeps a
    /// `Queue<StackItem>`, spends one `MaxStackSize` unit per compared sub item
    /// and one `MaxComparableSize` unit per non-`ByteString` sub item. The walk
    /// below spends the budget at exactly the same points, so a struct nested
    /// as deep as `MaxStackSize` compares without adding one native frame per
    /// level.
    fn equals_with_budget(&self, other: &Self, budget: &mut LimitBudget) -> VmResult<bool> {
        /// One unit of work, consumed in LIFO order.
        enum Step {
            /// Compare the two items.
            Compare(StackItem, StackItem),
            /// Spend the per-sub-item budget of a pair of struct elements.
            Account(StackItem, StackItem),
            /// Leave a struct pair that was being compared.
            Leave(usize, usize),
        }

        /// Outcome of one comparison, before the stack is updated.
        enum Outcome {
            Equal,
            NotEqual,
            Error(VmError),
            Struct {
                left: Box<StructItem>,
                right: Box<StructItem>,
                key: (usize, usize),
            },
        }

        /// Releases every struct pair that is still being compared.
        fn unwind(steps: &mut [Step], budget: &mut LimitBudget) {
            for step in steps.iter() {
                if let Step::Leave(a_id, b_id) = step {
                    budget.visited.remove(&(*a_id, *b_id));
                }
            }
        }

        /// The element-wise comparison of a struct pair for every item kind
        /// that is not descended into.
        fn inline_compare(left: &StackItem, right: &StackItem) -> bool {
            match (left, right) {
                (StackItem::Buffer(a), StackItem::Buffer(b)) => a.id() == b.id(),
                (StackItem::Array(a), StackItem::Array(b)) => a.id() == b.id(),
                (StackItem::Map(a), StackItem::Map(b)) => a.id() == b.id(),
                (StackItem::Boolean(a), StackItem::Boolean(b)) => a == b,
                (StackItem::Integer(a), StackItem::Integer(b)) => a.to_bigint() == b.to_bigint(),
                (StackItem::Pointer(a), StackItem::Pointer(b)) => a == b,
                (StackItem::InteropInterface(a), StackItem::InteropInterface(b)) => {
                    Arc::ptr_eq(a, b)
                }
                _ => false,
            }
        }

        fn compare_byte_strings(a: &[u8], b: &[u8], remaining: &mut i64) -> VmResult<bool> {
            if *remaining == 0 {
                return Err(VmError::invalid_operation_msg(
                    "The operand exceeds the maximum comparable size",
                ));
            }
            if a.len() as i64 > *remaining {
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{}",
                    a.len(),
                    *remaining
                )));
            }
            if b.len() as i64 > *remaining {
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{}",
                    b.len(),
                    *remaining
                )));
            }
            let compared = a.len().max(b.len()).max(1) as i64;
            *remaining -= compared;
            Ok(a == b)
        }

        let mut steps = vec![Step::Compare(self.clone(), other.clone())];
        while let Some(step) = steps.pop() {
            match step {
                Step::Leave(a_id, b_id) => {
                    budget.visited.remove(&(a_id, b_id));
                }
                Step::Account(left, right) => {
                    if budget.remaining_stack == 0 {
                        unwind(&mut steps, budget);
                        return Err(VmError::invalid_operation_msg(
                            "Too many struct items to compare in struct comparison",
                        ));
                    }
                    budget.remaining_stack -= 1;
                    if matches!(left, StackItem::ByteString(_)) {
                        // Byte strings spend the comparable budget inside
                        // their own comparison.
                        steps.push(Step::Compare(left, right));
                        continue;
                    }
                    if budget.remaining_comparable == 0 {
                        unwind(&mut steps, budget);
                        return Err(VmError::invalid_operation_msg(
                            "The operand exceeds the maximum comparable size in struct comparison",
                        ));
                    }
                    budget.remaining_comparable -= 1;
                    if matches!(left, StackItem::Struct(_)) {
                        steps.push(Step::Compare(left, right));
                    } else if !inline_compare(&left, &right) {
                        unwind(&mut steps, budget);
                        return Ok(false);
                    }
                }
                Step::Compare(left, right) => {
                    let outcome = match (&left, &right) {
                        (Self::Null, Self::Null) => Outcome::Equal,
                        (Self::Null, _) | (_, Self::Null) => Outcome::NotEqual,
                        (Self::Buffer(a), Self::Buffer(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Array(a), Self::Array(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Map(a), Self::Map(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Struct(a), Self::Struct(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                let key = (a.id(), b.id());
                                if budget.visited.insert(key) {
                                    Outcome::Struct {
                                        left: Box::new(a.clone()),
                                        right: Box::new(b.clone()),
                                        key,
                                    }
                                } else {
                                    // C# uses ReferenceEquals(a, b) to
                                    // short-circuit cycles inside structs; if
                                    // the same pair is re-entered treat it as
                                    // equal.
                                    Outcome::Equal
                                }
                            }
                        }
                        (Self::Boolean(a), Self::Boolean(b)) => {
                            if a == b {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Integer(a), Self::Integer(b)) => {
                            if a.to_bigint() == b.to_bigint() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::ByteString(a), Self::ByteString(b)) => {
                            match compare_byte_strings(a, b, &mut budget.remaining_comparable) {
                                Ok(true) => Outcome::Equal,
                                Ok(false) => Outcome::NotEqual,
                                Err(error) => Outcome::Error(error),
                            }
                        }
                        (Self::Pointer(a), Self::Pointer(b)) => {
                            if a == b {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::InteropInterface(a), Self::InteropInterface(b)) => {
                            if Arc::ptr_eq(a, b) {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        _ => Outcome::NotEqual,
                    };

                    match outcome {
                        Outcome::Equal => {}
                        Outcome::NotEqual => {
                            unwind(&mut steps, budget);
                            return Ok(false);
                        }
                        Outcome::Error(error) => {
                            unwind(&mut steps, budget);
                            return Err(error);
                        }
                        Outcome::Struct { left, right, key } => {
                            // The pair stays visited until all elements are
                            // compared; `unwind` releases it on an early
                            // return, exactly like the recursive form.
                            steps.push(Step::Leave(key.0, key.1));
                            let left_items = left.items();
                            let right_items = right.items();
                            if left_items.len() != right_items.len() {
                                unwind(&mut steps, budget);
                                return Ok(false);
                            }
                            let mut children = Vec::with_capacity(left_items.len());
                            for (left, right) in left_items.into_iter().zip(right_items) {
                                // `Account` spends the budget and then requests the
                                // comparison of the pair itself.
                                children.push(Step::Account(left, right));
                            }
                            steps.extend(children.into_iter().rev());
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    /// Checks if two stack items are equal with reference tracking to handle cycles.
    ///
    /// The pair is compared on an explicit frame stack, so an array or struct
    /// nested as deep as `MaxStackSize` compares without adding one native
    /// frame per level. The verdict — and the moments at which a pair leaves
    /// the visited set — is the same as the previous recursive form.
    fn equals_with_refs(
        &self,
        other: &Self,
        visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
    ) -> VmResult<bool> {
        /// A compound pair whose elements are being compared.
        enum Frame {
            /// An `Array` or `Struct`, compared element by element.
            Sequence {
                left: Vec<StackItem>,
                right: Vec<StackItem>,
                /// Index of the next element to compare.
                index: usize,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
            /// A `Map`, compared entry by entry: for every left entry, search
            /// the right entries for one whose key and value are both equal.
            Map {
                left: Vec<(StackItem, StackItem)>,
                right: Vec<(StackItem, StackItem)>,
                left_index: usize,
                right_index: usize,
                stage: Stage,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
        }

        #[derive(Clone, Copy)]
        enum Stage {
            Key,
            Value,
        }

        /// What the comparison of one pair produced.
        enum Outcome {
            /// Decided without descending. `release` is false where the
            /// recursive form returned early, which skipped removing the pair
            /// from the visited set.
            Decided {
                value: bool,
                release: bool,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
            /// Descend into `frame`, starting with the given pair.
            Descend {
                frame: Box<Frame>,
                first: (StackItem, StackItem),
            },
        }

        /// What an open frame needs next.
        enum Advance {
            /// Compare the given pair.
            Request(StackItem, StackItem),
            /// The frame is done; `release` tells whether its pair leaves the
            /// visited set.
            Done { release: bool },
        }

        fn frame_key(frame: &Frame) -> Option<(CompoundIdentity, CompoundIdentity)> {
            match frame {
                Frame::Sequence { key, .. } | Frame::Map { key, .. } => *key,
            }
        }

        /// Compares one pair. Growth of the visited set is undone by the
        /// caller, at the point the recursive form did it.
        fn compare_pair(
            left: &StackItem,
            right: &StackItem,
            visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
        ) -> Outcome {
            let key = match (compound_identity(left), compound_identity(right)) {
                (Some(left_id), Some(right_id)) => {
                    if visited.contains(&(left_id, right_id))
                        || visited.contains(&(right_id, left_id))
                    {
                        return Outcome::Decided {
                            value: true,
                            release: false,
                            key: None,
                        };
                    }
                    let key = (left_id, right_id);
                    visited.insert(key);
                    Some(key)
                }
                _ => None,
            };

            // C# Neo VM PrimitiveType.Equals is TYPE-STRICT: only items of the
            // SAME concrete primitive type (Integer, ByteString, Boolean)
            // compare equal, and only by value within that type. Cross-type
            // comparison (e.g. `Integer(1) == ByteString([0x01])`) returns
            // FALSE in C#, even when the byte representations match.
            let decided = |value: bool| Outcome::Decided {
                value,
                release: true,
                key,
            };
            match (left, right) {
                (StackItem::Null, StackItem::Null) => decided(true),
                // Buffer uses reference equality (compound type in C# Neo VM).
                (StackItem::Buffer(a), StackItem::Buffer(b)) => decided(a.id() == b.id()),
                (StackItem::Buffer(_), _) | (_, StackItem::Buffer(_)) => decided(false),
                (StackItem::Boolean(a), StackItem::Boolean(b)) => decided(a == b),
                (StackItem::Integer(a), StackItem::Integer(b)) => {
                    decided(a.to_bigint() == b.to_bigint())
                }
                (StackItem::ByteString(a), StackItem::ByteString(b)) => decided(a == b),
                (a, b)
                    if matches!(
                        a,
                        StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_)
                    ) && matches!(
                        b,
                        StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_)
                    ) =>
                {
                    decided(false)
                }
                (StackItem::Pointer(a), StackItem::Pointer(b)) => decided(a == b),
                (StackItem::InteropInterface(a), StackItem::InteropInterface(b)) => {
                    decided(Arc::ptr_eq(a, b))
                }
                (StackItem::Array(a), StackItem::Array(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_items = a.items();
                    let right_items = b.items();
                    let Some(first) = left_items
                        .first()
                        .map(|item| (item.clone(), right_items[0].clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Sequence {
                            left: left_items,
                            right: right_items,
                            index: 1,
                            key,
                        }),
                        first,
                    }
                }
                (StackItem::Struct(a), StackItem::Struct(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_items = a.items();
                    let right_items = b.items();
                    let Some(first) = left_items
                        .first()
                        .map(|item| (item.clone(), right_items[0].clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Sequence {
                            left: left_items,
                            right: right_items,
                            index: 1,
                            key,
                        }),
                        first,
                    }
                }
                (StackItem::Map(a), StackItem::Map(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_entries: Vec<_> = a
                        .items()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    let right_entries: Vec<_> = b
                        .items()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    let Some(first) = left_entries
                        .first()
                        .map(|(left_key, _)| (left_key.clone(), right_entries[0].0.clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Map {
                            left: left_entries,
                            right: right_entries,
                            left_index: 0,
                            right_index: 0,
                            stage: Stage::Key,
                            key,
                        }),
                        first,
                    }
                }
                _ => decided(false),
            }
        }

        /// Consumes the verdict of a child comparison and says what the frame
        /// needs next. A swallowed error becomes `false`, matching the
        /// `unwrap_or(false)` of the recursive map search.
        fn advance(frame: &mut Frame, result: &mut VmResult<bool>) -> Advance {
            match frame {
                Frame::Sequence {
                    left, right, index, ..
                } => {
                    if !matches!(result, Ok(true)) {
                        return Advance::Done { release: false };
                    }
                    if *index >= left.len() {
                        return Advance::Done { release: true };
                    }
                    let pair = (left[*index].clone(), right[*index].clone());
                    *index += 1;
                    Advance::Request(pair.0, pair.1)
                }
                Frame::Map {
                    left,
                    right,
                    left_index,
                    right_index,
                    stage,
                    ..
                } => {
                    if matches!(result, Ok(true)) {
                        match stage {
                            Stage::Key => {
                                *stage = Stage::Value;
                                let left_value = left[*left_index].1.clone();
                                let right_value = right[*right_index].1.clone();
                                return Advance::Request(left_value, right_value);
                            }
                            Stage::Value => {
                                // This left entry found a match.
                                *left_index += 1;
                                if *left_index >= left.len() {
                                    return Advance::Done { release: true };
                                }
                                *right_index = 0;
                            }
                        }
                    } else {
                        // The candidate failed, or its comparison errored and
                        // the error was swallowed: try the next candidate.
                        *result = Ok(false);
                        *right_index += 1;
                        if *right_index >= right.len() {
                            return Advance::Done { release: false };
                        }
                    }
                    *stage = Stage::Key;
                    let left_key = left[*left_index].0.clone();
                    let right_key = right[*right_index].0.clone();
                    Advance::Request(left_key, right_key)
                }
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some((self.clone(), other.clone()));
        let mut result: VmResult<bool> = Ok(true);

        loop {
            if let Some((left, right)) = pending.take() {
                match compare_pair(&left, &right, visited) {
                    Outcome::Decided {
                        value,
                        release,
                        key,
                    } => {
                        if release && let Some(key) = key {
                            visited.remove(&key);
                        }
                        result = Ok(value);
                    }
                    Outcome::Descend { frame, first } => {
                        frames.push(*frame);
                        pending = Some(first);
                        continue;
                    }
                }
            }

            let Some(frame) = frames.last_mut() else {
                return result;
            };
            match advance(frame, &mut result) {
                Advance::Request(left, right) => pending = Some((left, right)),
                Advance::Done { release } => {
                    let frame = frames.pop().expect("open frame");
                    if release && let Some(key) = frame_key(&frame) {
                        visited.remove(&key);
                    }
                }
            }
        }
    }
}
