// ContractTask tests are integration tests that would require tokio runtime.
// Since neo-primitives doesn't depend on tokio, we test the basic structure instead.

use neo_primitives::contract_task::ContractTask;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

// Helper to create a no-op waker for polling
fn noop_waker() -> Waker {
    use std::task::{RawWaker, RawWakerVTable};

    unsafe fn clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    fn noop_raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &NOOP_WAKER_VTABLE)
    }

    const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    unsafe { Waker::from_raw(noop_raw_waker()) }
}

#[test]
fn contract_task_completed_creates_ready_ok_future() {
    let mut task = ContractTask::completed();
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Ok(())) => (),
        _ => panic!("Expected Ready(Ok(()))"),
    }
}

#[test]
fn contract_task_failed_creates_ready_err_future() {
    let error_msg = "Task failed".to_string();
    let mut task = ContractTask::failed(error_msg.clone());
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Err(msg)) => assert_eq!(msg, error_msg),
        _ => panic!("Expected Ready(Err(_))"),
    }
}

#[test]
fn contract_task_new_with_ready_ok() {
    let mut task = ContractTask::new(async { Ok(()) });
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Ok(())) => (),
        _ => panic!("Expected Ready(Ok(()))"),
    }
}

#[test]
fn contract_task_new_with_ready_err() {
    let mut task = ContractTask::new(async { Err("error".to_string()) });
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Err(msg)) => assert_eq!(msg, "error"),
        _ => panic!("Expected Ready(Err(_))"),
    }
}

#[test]
fn contract_task_error_message_propagates() {
    let custom_error = "Custom validation failed";
    let mut task = ContractTask::failed(custom_error.to_string());
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Err(msg)) => assert_eq!(msg, custom_error),
        _ => panic!("Expected error message to propagate"),
    }
}

#[test]
fn contract_task_empty_error_message() {
    let mut task = ContractTask::failed(String::new());
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    match Pin::new(&mut task).poll(&mut cx) {
        Poll::Ready(Err(msg)) => assert_eq!(msg, ""),
        _ => panic!("Expected empty error message"),
    }
}

#[test]
fn contract_task_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<ContractTask>();
}

#[test]
fn contract_task_implements_future() {
    fn assert_future<T: Future>() {}
    assert_future::<ContractTask>();
}
