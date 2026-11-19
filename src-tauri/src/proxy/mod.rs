mod handler;
mod resolve;
mod server;
pub(crate) mod upstream;

pub use server::{start_with_settings, status_for, ProxyHandle, ProxyStatus};

#[cfg(test)]
mod tests;
