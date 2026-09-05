//! Small graph utilities used by VM reference tracking.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STACK_ITEM_ID: AtomicUsize = AtomicUsize::new(1);

/// Generates a unique identity for compound VM stack items.
pub fn next_stack_item_id() -> usize {
    NEXT_STACK_ITEM_ID.fetch_add(1, Ordering::SeqCst)
}

/// Tarjan's algorithm for finding strongly connected components.
///
/// This implementation intentionally uses `Vec`-backed maps so it remains
/// compatible with `no_std + alloc` targets without requiring hashing support.
pub struct Tarjan<T> {
    graph: Vec<(T, Vec<T>)>,
    index: Vec<(T, usize)>,
    lowlink: Vec<(T, usize)>,
    on_stack: Vec<T>,
    stack: Vec<T>,
    current_index: usize,
    components: Vec<Vec<T>>,
}

impl<T> Tarjan<T>
where
    T: Eq + Clone,
{
    /// Creates a new Tarjan algorithm instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            graph: Vec::new(),
            index: Vec::new(),
            lowlink: Vec::new(),
            on_stack: Vec::new(),
            stack: Vec::new(),
            current_index: 0,
            components: Vec::new(),
        }
    }

    /// Default implementation delegates to `new()`.
    #[must_use]
    pub const fn default_instance() -> Self {
        Self::new()
    }

    /// Adds a vertex to the graph.
    pub fn add_vertex(&mut self, vertex: T) {
        if !self.graph.iter().any(|(existing, _)| existing == &vertex) {
            self.graph.push((vertex, Vec::new()));
        }
    }

    /// Adds an edge from `from` to `to`.
    pub fn add_edge(&mut self, from: T, to: T) {
        self.add_vertex(from.clone());
        self.add_vertex(to.clone());

        if let Some((_, edges)) = self.graph.iter_mut().find(|(vertex, _)| vertex == &from) {
            edges.push(to);
        }
    }

    /// Finds all strongly connected components in the graph.
    pub fn find_components(&mut self) -> &[Vec<T>] {
        self.index.clear();
        self.lowlink.clear();
        self.on_stack.clear();
        self.stack.clear();
        self.current_index = 0;
        self.components.clear();

        for vertex in self
            .graph
            .iter()
            .map(|(vertex, _)| vertex.clone())
            .collect::<Vec<_>>()
        {
            if self.get_index(&vertex).is_none() {
                self.strong_connect(vertex);
            }
        }

        &self.components
    }

    fn strong_connect(&mut self, vertex: T) {
        self.set_index(vertex.clone(), self.current_index);
        self.set_lowlink(vertex.clone(), self.current_index);
        self.current_index += 1;
        self.stack.push(vertex.clone());
        self.on_stack.push(vertex.clone());

        if let Some(successors) = self.successors(&vertex) {
            for successor in successors {
                if self.get_index(&successor).is_none() {
                    self.strong_connect(successor.clone());

                    let vertex_lowlink = self.get_lowlink(&vertex).unwrap_or(0);
                    let successor_lowlink = self.get_lowlink(&successor).unwrap_or(0);
                    self.set_lowlink(vertex.clone(), vertex_lowlink.min(successor_lowlink));
                } else if self.on_stack.iter().any(|item| item == &successor) {
                    let vertex_lowlink = self.get_lowlink(&vertex).unwrap_or(0);
                    let successor_index = self.get_index(&successor).unwrap_or(0);
                    self.set_lowlink(vertex.clone(), vertex_lowlink.min(successor_index));
                }
            }
        }

        let vertex_index = self.get_index(&vertex).unwrap_or(0);
        let vertex_lowlink = self.get_lowlink(&vertex).unwrap_or(0);

        if vertex_index == vertex_lowlink {
            let mut component = Vec::new();
            loop {
                let Some(item) = self.stack.pop() else {
                    break;
                };
                self.remove_from_stack(&item);
                component.push(item.clone());
                if item == vertex {
                    break;
                }
            }
            self.components.push(component);
        }
    }

    fn successors(&self, vertex: &T) -> Option<Vec<T>> {
        self.graph
            .iter()
            .find(|(candidate, _)| candidate == vertex)
            .map(|(_, edges)| edges.clone())
    }

    fn get_index(&self, vertex: &T) -> Option<usize> {
        self.index
            .iter()
            .find(|(candidate, _)| candidate == vertex)
            .map(|(_, value)| *value)
    }

    fn set_index(&mut self, vertex: T, value: usize) {
        set_entry(&mut self.index, vertex, value);
    }

    fn get_lowlink(&self, vertex: &T) -> Option<usize> {
        self.lowlink
            .iter()
            .find(|(candidate, _)| candidate == vertex)
            .map(|(_, value)| *value)
    }

    fn set_lowlink(&mut self, vertex: T, value: usize) {
        set_entry(&mut self.lowlink, vertex, value);
    }

    fn remove_from_stack(&mut self, vertex: &T) {
        if let Some(position) = self
            .on_stack
            .iter()
            .position(|candidate| candidate == vertex)
        {
            self.on_stack.remove(position);
        }
    }
}

fn set_entry<T>(entries: &mut Vec<(T, usize)>, key: T, value: usize)
where
    T: Eq,
{
    if let Some((_, existing)) = entries.iter_mut().find(|(candidate, _)| candidate == &key) {
        *existing = value;
    } else {
        entries.push((key, value));
    }
}

impl<T> Default for Tarjan<T>
where
    T: Eq + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Tarjan;
    use alloc::vec;

    #[test]
    fn tarjan_finds_cycles_and_singletons() {
        let mut tarjan = Tarjan::new();
        tarjan.add_edge(1, 2);
        tarjan.add_edge(2, 3);
        tarjan.add_edge(3, 1);
        tarjan.add_edge(3, 4);
        tarjan.add_edge(4, 5);
        tarjan.add_edge(5, 4);
        tarjan.add_edge(5, 6);

        let components = tarjan.find_components();
        assert_eq!(components.len(), 3);
        assert!(components.iter().any(|component| {
            component.len() == 3
                && component.contains(&1)
                && component.contains(&2)
                && component.contains(&3)
        }));
        assert!(components.iter().any(|component| component.len() == 2
            && component.contains(&4)
            && component.contains(&5)));
        assert!(
            components
                .iter()
                .any(|component| component.as_slice() == [6])
        );
    }

    #[test]
    fn tarjan_handles_dag_as_singletons() {
        let mut tarjan = Tarjan::new();
        tarjan.add_edge(1, 2);
        tarjan.add_edge(2, 3);
        tarjan.add_edge(3, 4);

        let components = tarjan.find_components();
        assert_eq!(components.len(), 4);
        assert!(components.iter().all(|component| component.len() == 1));
    }

    #[test]
    fn tarjan_handles_single_vertex() {
        let mut tarjan = Tarjan::new();
        tarjan.add_vertex(1);

        assert_eq!(tarjan.find_components(), &[vec![1]]);
    }
}
