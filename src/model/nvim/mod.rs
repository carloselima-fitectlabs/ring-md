//! The embedded Neovim engine (F2): process, RPC, redraw decoding and input
//! encoding.

pub mod command;
pub mod fake;
pub mod input;
pub mod redraw;
pub mod rpc;
pub mod runtime;
pub mod session;
pub mod version;

use rmpv::Value;

pub use command::GuiCommand;
pub use session::{NvimSession, SpawnOptions};
pub use version::Version;

use crate::services::LocalBoxFuture;

/// Errors from starting or talking to Neovim.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NvimError {
    #[error(
        "`{program}` was not found. RingMD needs Neovim {} or newer.",
        version::MINIMUM
    )]
    NotFound { program: String },
    #[error(
        "Neovim {found} is too old. RingMD needs Neovim {} or newer.",
        version::MINIMUM
    )]
    TooOld { found: Version },
    #[error("could not read the Neovim version from `{0}`")]
    UnknownVersion(String),
    #[error("could not start Neovim: {0}")]
    Spawn(String),
    #[error("Neovim returned an error: {0}")]
    Rpc(String),
    #[error("the connection to Neovim is closed")]
    Closed,
}

/// Something that happened on the Neovim side.
#[derive(Debug, Clone, PartialEq)]
pub enum NvimEvent {
    /// A notification such as `redraw` or one of RingMD's `rpcnotify` calls.
    Notification { method: String, params: Vec<Value> },
    /// A request that must be answered with [`NvimApi::respond`].
    Request {
        id: u64,
        method: String,
        params: Vec<Value>,
    },
    /// The process ended. `code` is `None` when it was killed by a signal.
    Exited { code: Option<i32> },
}

/// The calls ViewModels make into Neovim. [`NvimSession`] is the real
/// implementation; [`fake::FakeNvim`] records calls for tests.
pub trait NvimApi {
    /// Fire-and-forget call.
    fn notify(&self, method: &str, args: Vec<Value>);

    /// Call and wait for the result. The request is sent immediately, so
    /// calls made afterwards reach Neovim after it.
    fn request(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> LocalBoxFuture<'static, Result<Value, NvimError>>;

    /// Answer a request received as [`NvimEvent::Request`].
    fn respond(&self, id: u64, result: Result<Value, Value>);

    /// `nvim_ui_attach` with the options RingMD needs (F2.R1).
    fn ui_attach(&self, cols: u32, rows: u32) -> LocalBoxFuture<'static, Result<Value, NvimError>> {
        let options = Value::Map(vec![
            ("rgb".into(), true.into()),
            ("ext_linegrid".into(), true.into()),
        ]);
        self.request("nvim_ui_attach", vec![cols.into(), rows.into(), options])
    }

    /// Send keys in Neovim's `<C-x>` notation (F2.R4).
    fn input(&self, keys: &str) {
        self.notify("nvim_input", vec![keys.into()]);
    }

    /// Send a mouse event (F2.R5).
    fn input_mouse(&self, event: &input::MouseEvent) {
        self.notify(
            "nvim_input_mouse",
            vec![
                event.button.as_str().into(),
                event.action.as_str().into(),
                event.modifiers.mouse_prefix().into(),
                0.into(),
                event.row.into(),
                event.col.into(),
            ],
        );
    }

    /// Ask Neovim to resize the grid (F2.R6).
    fn ui_try_resize(&self, cols: u32, rows: u32) {
        self.notify("nvim_ui_try_resize", vec![cols.into(), rows.into()]);
    }
}
