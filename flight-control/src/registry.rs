// SPDX-License-Identifier: MIT

use crate::{classify, HostError, SshRunner, Transport};
use flight_state::{HostId, ServerId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxError, TmuxRunner};
use std::collections::BTreeMap;

/// A runner of any kind, as the registry stores it.
pub type BoxedRunner = Box<dyn TmuxRunner + Send + Sync>;

pub(crate) struct Server {
    pub(crate) endpoint: TmuxEndpoint,
    pub(crate) tmux: Tmux<BoxedRunner>,
}

pub(crate) struct Host {
    pub(crate) transport: Transport,
    pub(crate) servers: BTreeMap<ServerId, Server>,
}

/// Routes operations by host identity. Knows hosts, transports and typed failures; knows
/// nothing about pane parsing or the tmux protocol (that is `flight-tmux`).
///
/// ```text
/// PaneRef -> HostId -> transport -> TmuxEndpoint -> flight-tmux
/// ```
#[derive(Default)]
pub struct HostRegistry {
    pub(crate) hosts: BTreeMap<HostId, Host>,
}

impl HostRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a host. Re-registering replaces it and its servers.
    pub fn add_host(&mut self, id: HostId, transport: Transport) {
        self.hosts.insert(
            id,
            Host {
                transport,
                servers: BTreeMap::new(),
            },
        );
    }

    /// Register a tmux server on a host, reached through the host's transport.
    pub fn add_server(
        &mut self,
        host: &HostId,
        server: ServerId,
        endpoint: TmuxEndpoint,
    ) -> Result<(), HostError> {
        let entry = self
            .hosts
            .get_mut(host)
            .ok_or_else(|| HostError::UnknownHost(host.clone()))?;
        let runner: BoxedRunner = match &entry.transport {
            Transport::Local => Box::new(SystemRunner::new(endpoint.clone())),
            Transport::Ssh { alias } => Box::new(SshRunner::new(alias, endpoint.clone())?),
        };
        entry.servers.insert(
            server,
            Server {
                endpoint,
                tmux: Tmux::with_runner(runner),
            },
        );
        Ok(())
    }

    /// Register a server with a caller-supplied runner: a future transport, or a fake in tests.
    /// `transport` classification still follows the host's registered [`Transport`].
    pub fn add_server_with_runner(
        &mut self,
        host: &HostId,
        server: ServerId,
        endpoint: TmuxEndpoint,
        runner: BoxedRunner,
    ) -> Result<(), HostError> {
        let entry = self
            .hosts
            .get_mut(host)
            .ok_or_else(|| HostError::UnknownHost(host.clone()))?;
        entry.servers.insert(
            server,
            Server {
                endpoint,
                tmux: Tmux::with_runner(runner),
            },
        );
        Ok(())
    }

    /// The endpoint a (host, server) maps to.
    pub fn endpoint(&self, host: &HostId, server: &ServerId) -> Result<&TmuxEndpoint, HostError> {
        Ok(&self.server(host, server)?.1.endpoint)
    }

    pub(crate) fn server(
        &self,
        host: &HostId,
        server: &ServerId,
    ) -> Result<(&Host, &Server), HostError> {
        let h = self
            .hosts
            .get(host)
            .ok_or_else(|| HostError::UnknownHost(host.clone()))?;
        let s = h
            .servers
            .get(server)
            .ok_or_else(|| HostError::UnknownServer(host.clone(), server.clone()))?;
        Ok((h, s))
    }

    /// Run `f` against a server's tmux, classifying any failure for its host's transport.
    pub(crate) fn call<T>(
        &self,
        host: &HostId,
        server: &ServerId,
        f: impl FnOnce(&Tmux<BoxedRunner>) -> Result<T, TmuxError>,
    ) -> Result<T, HostError> {
        let (h, s) = self.server(host, server)?;
        f(&s.tmux).map_err(|e| classify(&h.transport, &e))
    }
}
