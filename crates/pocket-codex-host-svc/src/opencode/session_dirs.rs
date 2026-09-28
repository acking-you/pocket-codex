//! Session working directories for the OpenCode meta service's
//! `/fs/thread-file`, read from the attached OpenCode server.

use std::sync::{Arc, RwLock};

use super::{Client, Error};
use crate::file_links::SessionDirResolver;

/// Resolves `session → location.directory` through the host-local,
/// authenticated [`Client`]. Clones share one client slot, so a host that
/// re-attaches after an OpenCode restart repoints every clone with
/// [`SessionDirs::set_client`].
#[derive(Clone, Debug)]
pub struct SessionDirs {
    client: Arc<RwLock<Client>>,
}

impl SessionDirs {
    /// Resolve through `client`.
    pub fn new(client: Client) -> Self {
        Self {
            client: Arc::new(RwLock::new(client)),
        }
    }

    /// Follow a re-attached OpenCode server.
    pub fn set_client(&self, client: Client) {
        *self
            .client
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = client;
    }

    fn current(&self) -> Client {
        self.client
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

#[async_trait::async_trait]
impl SessionDirResolver for SessionDirs {
    async fn session_dir(&self, session: &str) -> anyhow::Result<Option<String>> {
        match self.current().session(session).await {
            Ok(session) => Ok(Some(session.location.directory)),
            // A malformed id cannot name a session either.
            Err(Error::Rejected(404) | Error::InvalidInput) => Ok(None),
            Err(error) => Err(anyhow::anyhow!("resolving the OpenCode session: {error}")),
        }
    }
}
