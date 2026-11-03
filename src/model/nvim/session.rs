//! The Neovim child process and its msgpack-RPC transport (T1.1).
//!
//! Two background threads do the blocking I/O: a reader that decodes
//! messages from stdout and a writer that drains an outgoing queue into
//! stdin. Everything else happens on the caller's (GLib main) thread:
//! responses complete futures returned by [`NvimSession::request`], and
//! notifications/requests arrive on the event channel.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use rmpv::Value;

use super::rpc::{self, Message};
use super::{NvimApi, NvimError, NvimEvent};
use crate::services::LocalBoxFuture;

type Pending = Arc<Mutex<HashMap<u64, async_channel::Sender<Result<Value, Value>>>>>;

/// How to start Neovim.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// The `nvim` executable.
    pub program: String,
    /// Config passed with `-u` (F2.R2).
    pub init: PathBuf,
    /// File to open, if any.
    pub file: Option<PathBuf>,
}

impl SpawnOptions {
    pub fn new(init: impl Into<PathBuf>) -> Self {
        Self {
            program: "nvim".into(),
            init: init.into(),
            file: None,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.arg("--embed").arg("-u").arg(&self.init);
        if let Some(file) = &self.file {
            command.arg("--").arg(file);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        command
    }
}

/// A running `nvim --embed` process. Cheap to clone; the process is killed
/// when the last clone is dropped.
#[derive(Debug, Clone)]
pub struct NvimSession {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    outgoing: mpsc::Sender<Vec<u8>>,
    pending: Pending,
    next_id: Mutex<u64>,
    child: Arc<Mutex<Child>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl NvimSession {
    /// Check the version, start Neovim and return the session plus the
    /// channel its events arrive on.
    pub fn spawn(
        options: &SpawnOptions,
    ) -> Result<(Self, async_channel::Receiver<NvimEvent>), NvimError> {
        super::version::check(&options.program)?;

        let mut child = options
            .command()
            .spawn()
            .map_err(|e| NvimError::Spawn(e.to_string()))?;
        let stdin = child.stdin.take().ok_or(NvimError::Closed)?;
        let stdout = child.stdout.take().ok_or(NvimError::Closed)?;
        tracing::info!(pid = child.id(), ?options, "started neovim");

        let child = Arc::new(Mutex::new(child));
        let pending: Pending = Arc::default();
        let (events_tx, events_rx) = async_channel::unbounded();
        let (outgoing_tx, outgoing_rx) = mpsc::channel();

        spawn_thread("nvim-writer", move || write_loop(stdin, outgoing_rx))?;
        let reader_pending = pending.clone();
        let reader_child = child.clone();
        spawn_thread("nvim-reader", move || {
            read_loop(stdout, &reader_pending, &events_tx);
            // Fail any request still waiting for an answer.
            if let Ok(mut pending) = reader_pending.lock() {
                pending.clear();
            }
            let code = reader_child
                .lock()
                .ok()
                .and_then(|mut child| child.wait().ok())
                .and_then(|status| status.code());
            tracing::info!(?code, "neovim exited");
            let _ = events_tx.send_blocking(NvimEvent::Exited { code });
        })?;

        let session = Self {
            inner: Arc::new(Inner {
                outgoing: outgoing_tx,
                pending,
                next_id: Mutex::new(0),
                child,
            }),
        };
        Ok((session, events_rx))
    }

    fn send(&self, message: &Message) {
        if self.inner.outgoing.send(message.encode()).is_err() {
            tracing::warn!("dropping message: neovim writer has stopped");
        }
    }

    fn next_id(&self) -> u64 {
        let mut id = self
            .inner
            .next_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *id += 1;
        *id
    }
}

impl NvimApi for NvimSession {
    fn notify(&self, method: &str, args: Vec<Value>) {
        tracing::trace!(method, "notify");
        self.send(&Message::Notification {
            method: method.to_owned(),
            params: args,
        });
    }

    fn request(
        &self,
        method: &str,
        args: Vec<Value>,
    ) -> LocalBoxFuture<'static, Result<Value, NvimError>> {
        let id = self.next_id();
        let (tx, rx) = async_channel::bounded(1);
        if let Ok(mut pending) = self.inner.pending.lock() {
            pending.insert(id, tx);
        }
        tracing::trace!(method, id, "request");
        self.send(&Message::Request {
            id,
            method: method.to_owned(),
            params: args,
        });
        Box::pin(async move {
            match rx.recv().await {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => Err(NvimError::Rpc(rpc::error_text(&error))),
                Err(_) => Err(NvimError::Closed),
            }
        })
    }

    fn respond(&self, id: u64, result: Result<Value, Value>) {
        self.send(&Message::Response { id, result });
    }
}

fn spawn_thread(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), NvimError> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(f)
        .map(drop)
        .map_err(|e| NvimError::Spawn(e.to_string()))
}

fn write_loop(stdin: ChildStdin, outgoing: mpsc::Receiver<Vec<u8>>) {
    let mut stdin = BufWriter::new(stdin);
    while let Ok(bytes) = outgoing.recv() {
        let mut result = stdin.write_all(&bytes);
        // Batch whatever else is already queued before flushing.
        while result.is_ok() {
            match outgoing.try_recv() {
                Ok(more) => result = stdin.write_all(&more),
                Err(_) => break,
            }
        }
        if result.and_then(|()| stdin.flush()).is_err() {
            break;
        }
    }
}

fn read_loop(stdout: ChildStdout, pending: &Pending, events: &async_channel::Sender<NvimEvent>) {
    let mut stdout = BufReader::new(stdout);
    loop {
        let value = match rmpv::decode::read_value(&mut stdout) {
            Ok(value) => value,
            Err(e) => {
                tracing::debug!(error = %e, "neovim stdout closed");
                return;
            }
        };
        let event = match Message::from_value(value) {
            Ok(Message::Response { id, result }) => {
                let waiter = pending.lock().ok().and_then(|mut p| p.remove(&id));
                match waiter {
                    Some(waiter) => {
                        let _ = waiter.try_send(result);
                    }
                    None => tracing::warn!(id, "response to unknown request"),
                }
                continue;
            }
            Ok(Message::Notification { method, params }) => {
                NvimEvent::Notification { method, params }
            }
            Ok(Message::Request { id, method, params }) => {
                NvimEvent::Request { id, method, params }
            }
            Err(e) => {
                tracing::warn!(error = %e, "ignoring message");
                continue;
            }
        };
        if events.send_blocking(event).is_err() {
            return;
        }
    }
}
