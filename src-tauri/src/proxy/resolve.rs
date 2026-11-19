use std::net::SocketAddr;
use std::sync::Arc;

use crate::error::AppError;

#[derive(Clone)]
pub enum HostResolver {
    System,
    #[cfg(test)]
    Fixed(Arc<Vec<SocketAddr>>),
}

impl HostResolver {
    pub fn system() -> Self {
        Self::System
    }

    #[cfg(test)]
    pub fn fixed(addrs: Vec<SocketAddr>) -> Self {
        Self::Fixed(Arc::new(addrs))
    }

    pub async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, AppError> {
        match self {
            Self::System => {
                let addrs = tokio::net::lookup_host((host, port))
                    .await
                    .map_err(|err| AppError::message(format!("could not resolve {host}: {err}")))?;
                Ok(addrs.collect())
            }
            #[cfg(test)]
            Self::Fixed(addrs) => {
                let _ = (host, port);
                Ok(addrs.as_ref().clone())
            }
        }
    }
}
