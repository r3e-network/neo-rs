use super::*;

/// Renders the item tree on an explicit work stack.
///
/// A derived formatter adds one native frame per nesting level, so an item
/// nested as deep as `MaxStackSize` (a legal depth) overflowed a 1 MiB thread
/// stack. The emitted text — including the wrapper shapes of the compound
/// items (`Type { inner: Mutex { data: TypeInner { .. } } }`) and their ids —
/// is the same as the previous derived output, so log and error-message text
/// does not change.
impl fmt::Debug for StackItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// One print job; the tree is walked by popping these.
        enum Task {
            /// Print a stack item at the given indentation column.
            Item(StackItem, usize),
            /// Already-rendered text.
            Text(String),
            /// A numeric field value.
            Num(isize),
            /// A boolean field value.
            Bool(bool),
            /// Newline followed by `n` spaces.
            Indent(usize),
        }

        fn text(value: &str) -> Task {
            Task::Text(value.to_string())
        }

        /// `value` with every line but the first shifted right by `indent`.
        ///
        /// The leaves are formatted by their own implementations at base
        /// indentation and then moved to their position in the tree.
        fn shift_lines(value: &str, indent: usize) -> String {
            if indent == 0 {
                return value.to_string();
            }
            let pad = " ".repeat(indent);
            let mut out = String::with_capacity(value.len() + pad.len() * 8);
            for (index, line) in value.split('\n').enumerate() {
                if index > 0 {
                    out.push('\n');
                    out.push_str(&pad);
                }
                out.push_str(line);
            }
            out
        }

        /// `Prefix(payload)` laid out the way the derived implementation lays
        /// it out at `indent`.
        fn leaf(prefix: &str, payload: &dyn fmt::Debug, pretty: bool, indent: usize) -> String {
            if !pretty {
                return format!("{prefix}({payload:?})");
            }
            let body = shift_lines(&format!("{payload:#?}"), indent + 4);
            format!(
                "{prefix}(\n{}{body},\n{})",
                " ".repeat(indent + 4),
                " ".repeat(indent)
            )
        }

        /// The elements of a list, as `Vec`'s implementation prints them.
        fn push_list(out: &mut Vec<Task>, items: Vec<StackItem>, indent: usize, pretty: bool) {
            if items.is_empty() {
                out.push(text("[]"));
                return;
            }
            out.push(text("["));
            if pretty {
                for item in items {
                    out.push(Task::Indent(indent + 4));
                    out.push(Task::Item(item, indent + 4));
                    out.push(text(","));
                }
                out.push(Task::Indent(indent));
            } else {
                for (index, item) in items.into_iter().enumerate() {
                    if index > 0 {
                        out.push(text(", "));
                    }
                    out.push(Task::Item(item, 0));
                }
            }
            out.push(text("]"));
        }

        fn push_num_field(out: &mut Vec<Task>, name: &str, value: isize, indent: usize) {
            out.push(Task::Indent(indent));
            out.push(Task::Text(format!("{name}: ")));
            out.push(Task::Num(value));
            out.push(text(","));
        }

        /// One `Array`/`Struct` node, in print order.
        fn list_node(
            kind: &str,
            items: Vec<StackItem>,
            id: usize,
            stack_references: isize,
            is_read_only: bool,
            indent: usize,
            pretty: bool,
        ) -> Vec<Task> {
            let mut out = Vec::new();
            if pretty {
                out.push(Task::Text(format!("{kind}(")));
                out.push(Task::Indent(indent + 4));
                out.push(Task::Text(format!("{kind} {{")));
                out.push(Task::Indent(indent + 8));
                out.push(text("inner: Mutex {"));
                out.push(Task::Indent(indent + 12));
                out.push(text("data: CompoundInner {"));
                out.push(Task::Indent(indent + 16));
                out.push(text("items: "));
                push_list(&mut out, items, indent + 16, pretty);
                out.push(text(","));
                push_num_field(&mut out, "stack_references", stack_references, indent + 16);
                push_num_field(&mut out, "id", id as isize, indent + 16);
                out.push(Task::Indent(indent + 16));
                out.push(text("is_read_only: "));
                out.push(Task::Bool(is_read_only));
                out.push(text(","));
                out.push(Task::Indent(indent + 12));
                out.push(text("},"));
                out.push(Task::Indent(indent + 8));
                out.push(text("},"));
                out.push(Task::Indent(indent + 4));
                out.push(text("},"));
                out.push(Task::Indent(indent));
                out.push(text(")"));
                return out;
            }

            out.push(Task::Text(format!(
                "{kind}({kind} {{ inner: Mutex {{ data: CompoundInner {{ items: "
            )));
            push_list(&mut out, items, 0, pretty);
            out.push(text(", stack_references: "));
            out.push(Task::Num(stack_references));
            out.push(text(", id: "));
            out.push(Task::Num(id as isize));
            out.push(text(", is_read_only: "));
            out.push(Task::Bool(is_read_only));
            out.push(text(" } } })"));
            out
        }

        /// One `Map` node, in print order.
        fn map_node(
            entries: Vec<(StackItem, StackItem)>,
            id: usize,
            stack_references: isize,
            is_read_only: bool,
            indent: usize,
            pretty: bool,
        ) -> Vec<Task> {
            let mut out = Vec::new();
            if pretty {
                out.push(text("Map("));
                out.push(Task::Indent(indent + 4));
                out.push(text("Map {"));
                out.push(Task::Indent(indent + 8));
                out.push(text("inner: Mutex {"));
                out.push(Task::Indent(indent + 12));
                out.push(text("data: MapInner {"));
                out.push(Task::Indent(indent + 16));
                out.push(text("items: "));
                if entries.is_empty() {
                    out.push(text("{}"));
                } else {
                    out.push(text("{"));
                    for (key, value) in entries {
                        out.push(Task::Indent(indent + 20));
                        out.push(Task::Item(key, indent + 20));
                        out.push(text(": "));
                        out.push(Task::Item(value, indent + 20));
                        out.push(text(","));
                    }
                    out.push(Task::Indent(indent + 16));
                    out.push(text("}"));
                }
                out.push(text(","));
                push_num_field(&mut out, "id", id as isize, indent + 16);
                push_num_field(&mut out, "stack_references", stack_references, indent + 16);
                out.push(Task::Indent(indent + 16));
                out.push(text("is_read_only: "));
                out.push(Task::Bool(is_read_only));
                out.push(text(","));
                out.push(Task::Indent(indent + 12));
                out.push(text("},"));
                out.push(Task::Indent(indent + 8));
                out.push(text("},"));
                out.push(Task::Indent(indent + 4));
                out.push(text("},"));
                out.push(Task::Indent(indent));
                out.push(text(")"));
                return out;
            }

            out.push(text("Map(Map { inner: Mutex { data: MapInner { items: {"));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(text(", "));
                }
                out.push(Task::Item(key, 0));
                out.push(text(": "));
                out.push(Task::Item(value, 0));
            }
            out.push(text("}, id: "));
            out.push(Task::Num(id as isize));
            out.push(text(", stack_references: "));
            out.push(Task::Num(stack_references));
            out.push(text(", is_read_only: "));
            out.push(Task::Bool(is_read_only));
            out.push(text(" } } })"));
            out
        }

        let pretty = f.alternate();
        let mut tasks = vec![Task::Item(self.clone(), 0)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Text(value) => f.write_str(&value)?,
                Task::Num(value) => write!(f, "{value}")?,
                Task::Bool(value) => write!(f, "{value}")?,
                Task::Indent(spaces) => {
                    f.write_str("\n")?;
                    for _ in 0..spaces {
                        f.write_str(" ")?;
                    }
                }
                Task::Item(item, indent) => {
                    let tokens = match &item {
                        Self::Array(array) => {
                            let (items, id, stack_references, is_read_only) =
                                array.debug_snapshot();
                            Some(list_node(
                                "Array",
                                items,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Struct(structure) => {
                            let (items, id, stack_references, is_read_only) =
                                structure.debug_snapshot();
                            Some(list_node(
                                "Struct",
                                items,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Map(map) => {
                            let (entries, id, stack_references, is_read_only) =
                                map.debug_snapshot();
                            let entries = entries
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone()))
                                .collect();
                            Some(map_node(
                                entries,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Null => {
                            f.write_str("Null")?;
                            None
                        }
                        Self::Boolean(value) => {
                            f.write_str(&leaf("Boolean", value, pretty, indent))?;
                            None
                        }
                        Self::Integer(value) => {
                            f.write_str(&leaf("Integer", value, pretty, indent))?;
                            None
                        }
                        Self::ByteString(value) => {
                            f.write_str(&leaf("ByteString", value, pretty, indent))?;
                            None
                        }
                        Self::Buffer(value) => {
                            f.write_str(&leaf("Buffer", value, pretty, indent))?;
                            None
                        }
                        Self::Pointer(value) => {
                            f.write_str(&leaf("Pointer", value, pretty, indent))?;
                            None
                        }
                        Self::InteropInterface(value) => {
                            f.write_str(&leaf("InteropInterface", value, pretty, indent))?;
                            None
                        }
                    };
                    if let Some(tokens) = tokens {
                        tasks.extend(tokens.into_iter().rev());
                    }
                }
            }
        }
        Ok(())
    }
}
