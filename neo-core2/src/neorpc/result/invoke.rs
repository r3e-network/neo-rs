use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use crate::core::state;
use crate::core::storage::dboper;
use crate::core::transaction;
use crate::vm::invocations;
use crate::vm::stackitem::{self, StackItem};

#[derive(Serialize, Deserialize)]
pub struct Invoke {
    state: String,
    gas_consumed: i64,
    script: Vec<u8>,
    stack: Vec<StackItem>,
    fault_exception: String,
    notifications: Vec<state::NotificationEvent>,
    transaction: Option<transaction::Transaction>,
    diagnostics: Option<InvokeDiag>,
    session: Uuid,
}

#[derive(Serialize, Deserialize)]
pub struct InvokeDiag {
    changes: Vec<dboper::Operation>,
    invocations: Vec<invocations::Tree>,
}

#[derive(Serialize, Deserialize)]
pub struct InvokeAux {
    state: String,
    gas_consumed: i64,
    script: Vec<u8>,
    stack: Value,
    fault_exception: Option<String>,
    notifications: Vec<state::NotificationEvent>,
    transaction: Option<Vec<u8>>,
    diagnostics: Option<InvokeDiag>,
    session: Option<String>,
}

const ITERATOR_INTERFACE_NAME: &str = "IIterator";

#[derive(Serialize, Deserialize)]
pub struct IteratorAux {
    #[serde(rename = "type")]
    type_: String,
    interface: Option<String>,
    id: Option<String>,
    value: Option<Vec<Value>>,
    truncated: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct Iterator {
    id: Option<Uuid>,
    values: Option<Vec<StackItem>>,
    truncated: bool,
}

impl Iterator {
    pub fn new() -> Self {
        Self {
            id: None,
            values: None,
            truncated: false,
        }
    }
}

impl Serialize for Iterator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut iaux = IteratorAux {
            type_: stackitem::InteropT.to_string(),
            interface: None,
            id: None,
            value: None,
            truncated: None,
        };

        if let Some(id) = &self.id {
            iaux.interface = Some(ITERATOR_INTERFACE_NAME.to_string());
            iaux.id = Some(id.to_string());
        } else if let Some(values) = &self.values {
            let mut value = Vec::with_capacity(values.len());
            for v in values {
                value.push(stackitem::to_json_with_types(v).map_err(serde::ser::Error::custom)?);
            }
            iaux.value = Some(value);
            iaux.truncated = Some(self.truncated);
        }

        iaux.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Iterator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let iaux = IteratorAux::deserialize(deserializer)?;
        let mut iter = Iterator::new();

        if let Some(interface) = iaux.interface {
            if interface != ITERATOR_INTERFACE_NAME {
                return Err(serde::de::Error::custom(format!("unknown InteropInterface: {}", interface)));
            }
            if let Some(id) = iaux.id {
                iter.id = Some(Uuid::parse_str(&id).map_err(serde::de::Error::custom)?);
            }
        } else if let Some(values) = iaux.value {
            let mut stack_items = Vec::with_capacity(values.len());
            for v in values {
                stack_items.push(stackitem::from_json_with_types(&v).map_err(serde::de::Error::custom)?);
            }
            iter.values = Some(stack_items);
            iter.truncated = iaux.truncated.unwrap_or(false);
        }

        Ok(iter)
    }
}

impl Serialize for Invoke {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut arr = Vec::with_capacity(self.stack.len());
        let mut fault_sep = "";
        let mut fault_exception = self.fault_exception.clone();

        for item in &self.stack {
            let data = if item.type_() == stackitem::InteropT {
                if let Some(iter) = item.value().downcast_ref::<Iterator>() {
                    serde_json::to_value(iter).map_err(serde::ser::Error::custom)?
                } else {
                    stackitem::to_json_with_types(item).map_err(serde::ser::Error::custom)?
                }
            } else {
                stackitem::to_json_with_types(item).map_err(serde::ser::Error::custom)?
            };

            arr.push(data);
        }

        let stack = serde_json::to_value(&arr).map_err(serde::ser::Error::custom)?;

        let txbytes = self.transaction.as_ref().map(|tx| tx.to_bytes());
        let session_id = if self.session != Uuid::nil() {
            Some(self.session.to_string())
        } else {
            None
        };

        let aux = InvokeAux {
            state: self.state.clone(),
            gas_consumed: self.gas_consumed,
            script: self.script.clone(),
            stack,
            fault_exception: if !self.fault_exception.is_empty() {
                Some(fault_exception)
            } else {
                None
            },
            notifications: self.notifications.clone(),
            transaction: txbytes,
            diagnostics: self.diagnostics.clone(),
            session: session_id,
        };

        aux.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Invoke {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let aux = InvokeAux::deserialize(deserializer)?;

        let session = if let Some(session) = aux.session {
            Uuid::parse_str(&session).map_err(serde::de::Error::custom)?
        } else {
            Uuid::nil()
        };

        let mut stack = Vec::with_capacity(aux.stack.as_array().unwrap().len());
        for item in aux.stack.as_array().unwrap() {
            let stack_item = stackitem::from_json_with_types(item).map_err(serde::de::Error::custom)?;
            if stack_item.type_() == stackitem::InteropT {
                let iter: Iterator = serde_json::from_value(item.clone()).map_err(serde::de::Error::custom)?;
                stack.push(stackitem::new_interop(iter));
            } else {
                stack.push(stack_item);
            }
        }

        let transaction = if let Some(txbytes) = aux.transaction {
            Some(transaction::Transaction::from_bytes(&txbytes).map_err(serde::de::Error::custom)?)
        } else {
            None
        };

        Ok(Invoke {
            state: aux.state,
            gas_consumed: aux.gas_consumed,
            script: aux.script,
            stack,
            fault_exception: aux.fault_exception.unwrap_or_default(),
            notifications: aux.notifications,
            transaction,
            diagnostics: aux.diagnostics,
            session,
        })
    }
}

pub fn app_exec_to_invocation(aer: &state::AppExecResult, err: Option<&str>) -> Result<Invoke, &str> {
    if let Some(e) = err {
        return Err(e);
    }

    Ok(Invoke {
        state: aer.vm_state.to_string(),
        gas_consumed: aer.gas_consumed,
        stack: aer.stack.clone(),
        fault_exception: aer.fault_exception.clone(),
        notifications: aer.events.clone(),
        transaction: None,
        diagnostics: None,
        session: Uuid::nil(),
    })
}
