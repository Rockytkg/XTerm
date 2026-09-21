mod common;
mod rdp;
mod serial;
mod ssh;
mod telnet;
mod vnc;

pub(crate) use common::protocol_registry;
