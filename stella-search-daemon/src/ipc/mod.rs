//! IPC (Inter-Process Communication) module
//!
//! Handles communication between the service and clients via named pipes (Windows)
//! or Unix domain sockets (Linux).

mod protocol;
mod server;
mod client;

pub use protocol::*;
pub use server::IpcServer;
pub use client::IpcClient;
