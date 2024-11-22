use std::marker::PhantomData;
use crate::neo_contract::contract_task_awaiter::ContractTaskAwaiter;

pub struct ContractTaskMethodBuilder<T = ()> {
    task: Option<ContractTask<T>>,
}

impl<T> ContractTaskMethodBuilder<T> {
    pub fn new() -> Self {
        Self { task: None }
    }

    pub fn task(&mut self) -> &mut ContractTask<T> {
        self.task.get_or_insert_with(ContractTask::new)
    }

    pub fn create() -> Self {
        Self::new()
    }

    pub fn set_exception(&mut self, exception: Box<dyn std::error::Error>) {
        self.task().get_awaiter().set_exception(exception);
    }

    pub fn set_result(&mut self, result: T) {
        self.task().get_awaiter().set_result(result);
    }

    pub fn await_on_completed<TAwaiter, TStateMachine>(
        &mut self,
        awaiter: &mut TAwaiter,
        state_machine: &mut TStateMachine,
    ) where
        TAwaiter: NotifyCompletion,
        TStateMachine: AsyncStateMachine,
    {
        awaiter.on_completed(Box::new(move || state_machine.move_next()));
    }

    pub fn await_unsafe_on_completed<TAwaiter, TStateMachine>(
        &mut self,
        awaiter: &mut TAwaiter,
        state_machine: &mut TStateMachine,
    ) where
        TAwaiter: CriticalNotifyCompletion,
        TStateMachine: AsyncStateMachine,
    {
        awaiter.on_completed(Box::new(move || state_machine.move_next()));
    }

    pub fn start<TStateMachine>(&mut self, state_machine: &mut TStateMachine)
    where
        TStateMachine: AsyncStateMachine,
    {
        state_machine.move_next();
    }

    pub fn set_state_machine(&mut self, _state_machine: Box<dyn AsyncStateMachine>) {
        // This method is intentionally left empty in the original C# code
    }
}

pub struct ContractTask<T = ()> {
    _phantom: PhantomData<T>,
}

impl<T> ContractTask<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    pub fn get_awaiter(&mut self) -> ContractTaskAwaiter<T> {
        ContractTaskAwaiter::new()
    }
}

pub trait NotifyCompletion {
    fn on_completed(&mut self, continuation: Box<dyn FnOnce()>);
}

pub trait CriticalNotifyCompletion: NotifyCompletion {}

pub trait AsyncStateMachine {
    fn move_next(&mut self);
}
