//! Cross-platform local IPC for the dictation daemon.
//!
//! The CLI (`flowoss trigger`, `daemon`, …) and the desktop app speak a small
//! line-based protocol over a local rendezvous socket. On Linux this is a Unix
//! domain socket in the abstract namespace; on Windows it is a named pipe.
//! Both are provided uniformly by `interprocess` so the rest of the code is
//! platform-agnostic.

use std::io;

use interprocess::local_socket::{
    prelude::*, GenericNamespaced, Listener, ListenerOptions, Stream,
};

/// Rendezvous name shared by the daemon and its clients. `GenericNamespaced`
/// maps this to `\\.\pipe\flowoss.sock` on Windows and an abstract-namespace
/// Unix socket on Linux.
pub const SOCKET_NAME: &str = "flowoss.sock";

/// Human-readable identifier for error messages.
pub fn socket_display() -> &'static str {
    SOCKET_NAME
}

/// Connect to a running daemon.
pub fn connect() -> io::Result<Stream> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    Stream::connect(name)
}

/// Bind the daemon's listener. Fails if another daemon already holds the name.
pub fn bind() -> io::Result<Listener> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    ListenerOptions::new().name(name).create_sync()
}

/// True if a daemon is currently accepting connections.
pub fn is_running() -> bool {
    connect().is_ok()
}
