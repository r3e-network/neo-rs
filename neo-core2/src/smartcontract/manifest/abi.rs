use std::cmp::Ordering;
use std::error::Error;
use crate::vm::stackitem::{self, Item};

pub const METHOD_INIT: &str = "_initialize";
pub const METHOD_DEPLOY: &str = "_deploy";
pub const METHOD_VERIFY: &str = "verify";
pub const METHOD_ON_NEP17_PAYMENT: &str = "onNEP17Payment";
pub const METHOD_ON_NEP11_PAYMENT: &str = "onNEP11Payment";

#[derive(Debug, Clone)]
pub struct ABI {
    pub methods: Vec<Method>,
    pub events: Vec<Event>,
}

impl ABI {
    pub fn get_method(&self, name: &str, param_count: i32) -> Option<&Method> {
        self.methods.iter().find(|m| {
            m.name == name && (param_count == -1 || m.parameters.len() as i32 == param_count)
        })
    }

    pub fn get_event(&self, name: &str) -> Option<&Event> {
        self.events.iter().find(|e| e.name == name)
    }

    pub fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        if self.methods.is_empty() {
            return Err("no methods".into());
        }

        for method in &self.methods {
            method.is_valid().map_err(|e| {
                format!("method \"{}\"/{}:{}", method.name, method.parameters.len(), e)
            })?;
        }

        if has_duplicates(&self.methods, |a, b| {
            a.name.cmp(&b.name).then(a.parameters.len().cmp(&b.parameters.len()))
        }) {
            return Err("duplicate method specifications".into());
        }

        for event in &self.events {
            event.is_valid().map_err(|e| {
                format!("event \"{}\"/{}:{}", event.name, event.parameters.len(), e)
            })?;
        }

        if has_duplicates(&self.events, |a, b| a.name.cmp(&b.name)) {
            return Err("duplicate event names".into());
        }

        Ok(())
    }

    pub fn to_stack_item(&self) -> Item {
        let methods: Vec<Item> = self.methods.iter().map(Method::to_stack_item).collect();
        let events: Vec<Item> = self.events.iter().map(Event::to_stack_item).collect();

        stackitem::Struct::new(vec![
            stackitem::Array::new(methods).into(),
            stackitem::Array::new(events).into(),
        ]).into()
    }

    pub fn from_stack_item(&mut self, item: &Item) -> Result<(), Box<dyn Error>> {
        let struct_item = item.as_struct().ok_or("invalid ABI stackitem type")?;
        let items = struct_item.value();

        if items.len() != 2 {
            return Err("invalid ABI stackitem length".into());
        }

        let methods = items[0].as_array().ok_or("invalid Methods stackitem type")?;
        self.methods = methods
            .iter()
            .map(|m| Method::from_stack_item(m))
            .collect::<Result<_, _>>()?;

        let events = items[1].as_array().ok_or("invalid Events stackitem type")?;
        self.events = events
            .iter()
            .map(|e| Event::from_stack_item(e))
            .collect::<Result<_, _>>()?;

        Ok(())
    }
}

fn has_duplicates<T, F>(slice: &[T], compare: F) -> bool
where
    F: Fn(&T, &T) -> Ordering,
{
    for i in 1..slice.len() {
        if compare(&slice[i - 1], &slice[i]) == Ordering::Equal {
            return true;
        }
    }
    false
}
