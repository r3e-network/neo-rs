use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::rc::Rc;

pub struct ContractTaskAwaiter {
    continuation: RefCell<Option<Box<dyn FnOnce()>>>,
    exception: RefCell<Option<Box<dyn std::error::Error>>>,
    is_completed: RefCell<bool>,
}

impl ContractTaskAwaiter {
    pub fn new() -> Self {
        Self {
            continuation: RefCell::new(None),
            exception: RefCell::new(None),
            is_completed: RefCell::new(false),
        }
    }

    pub fn is_completed(&self) -> bool {
        *self.is_completed.borrow()
    }

    pub fn get_result(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(e) = self.exception.borrow().as_ref() {
            Err(e.clone())
        } else {
            Ok(())
        }
    }

    pub fn set_result(&self) {
        self.run_continuation();
    }

    pub fn set_result_with_engine(&self, _engine: &ApplicationEngine) {
        self.set_result();
    }

    pub fn set_exception(&self, exception: Box<dyn std::error::Error>) {
        *self.exception.borrow_mut() = Some(exception);
        self.run_continuation();
    }

    pub fn on_completed<F: FnOnce() + 'static>(&self, continuation: F) {
        *self.continuation.borrow_mut() = Some(Box::new(continuation));
    }

    fn run_continuation(&self) {
        *self.is_completed.borrow_mut() = true;
        if let Some(c) = self.continuation.borrow_mut().take() {
            c();
        }
    }
}

pub struct ContractTaskAwaiter<T> {
    inner: ContractTaskAwaiter,
    result: RefCell<Option<T>>,
}

impl<T> ContractTaskAwaiter<T> {
    pub fn new() -> Self {
        Self {
            inner: ContractTaskAwaiter::new(),
            result: RefCell::new(None),
        }
    }

    pub fn get_result(&self) -> Result<T, Box<dyn std::error::Error>>
    where
        T: Clone,
    {
        self.inner.get_result()?;
        Ok(self.result.borrow().clone().unwrap())
    }

    pub fn set_result(&self, result: T) {
        *self.result.borrow_mut() = Some(result);
        self.inner.run_continuation();
    }

    pub fn set_result_with_engine(&self, engine: &ApplicationEngine) {
        let result = engine.convert(engine.pop(), &InteropParameterDescriptor::new::<T>());
        self.set_result(result);
    }
}

impl<T> std::ops::Deref for ContractTaskAwaiter<T> {
    type Target = ContractTaskAwaiter;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
