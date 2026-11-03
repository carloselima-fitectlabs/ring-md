//! A recording stand-in for Neovim, used by ViewModel tests (rule A6).

use std::cell::RefCell;

use rmpv::Value;

use super::{NvimApi, NvimError};
use crate::services::LocalBoxFuture;

/// A call made through [`NvimApi`].
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Notify(String, Vec<Value>),
    Request(String, Vec<Value>),
    Respond(u64, Result<Value, Value>),
}

/// Records every call; requests succeed with `nil` unless a reply is set
/// with [`FakeNvim::set_response`].
#[derive(Debug, Default)]
pub struct FakeNvim {
    calls: RefCell<Vec<Call>>,
    response: RefCell<Option<Value>>,
}

impl FakeNvim {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every later request with `value`.
    pub fn set_response(&self, value: Value) {
        self.response.replace(Some(value));
    }

    /// Take the calls recorded so far.
    pub fn take_calls(&self) -> Vec<Call> {
        self.calls.take()
    }

    /// The keys sent with `nvim_input`, in order.
    pub fn take_input(&self) -> Vec<String> {
        self.take_calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Notify(method, args) if method == "nvim_input" => {
                    args.first().and_then(|v| v.as_str()).map(str::to_owned)
                }
                _ => None,
            })
            .collect()
    }
}

impl NvimApi for FakeNvim {
    fn notify(&self, method: &str, args: Vec<Value>) {
        self.calls
            .borrow_mut()
            .push(Call::Notify(method.to_owned(), args));
    }

    fn request(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> LocalBoxFuture<'static, Result<Value, NvimError>> {
        self.calls
            .borrow_mut()
            .push(Call::Request(method.to_owned(), args));
        let response = self.response.borrow().clone().unwrap_or(Value::Nil);
        Box::pin(async move { Ok(response) })
    }

    fn respond(&self, id: u64, result: Result<Value, Value>) {
        self.calls.borrow_mut().push(Call::Respond(id, result));
    }
}
