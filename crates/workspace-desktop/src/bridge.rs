use async_channel::Receiver;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::PathBuf;
use workspace_core::{ask::Ask, *};
use workspace_host::service::Service;

/// Waits for the host's answer to `ask`, before the app starts.
fn answer<R: DeserializeOwned>(service: &Service, ask: Ask<R>) -> anyhow::Result<R> {
    let reply = service
        .request(ask.command().clone())
        .map_err(anyhow::Error::msg)?;
    let value = reply.recv_blocking()?.map_err(anyhow::Error::msg)?;
    ask.decode(value).map_err(anyhow::Error::msg)
}

#[derive(Clone)]
pub(crate) struct Bridge {
    service: Result<Service, String>,
}
impl Bridge {
    pub(crate) fn start(home: PathBuf, demo_repository: Option<String>) -> Self {
        let result = (|| -> anyhow::Result<Service> {
            let binary = std::env::current_exe()?;
            let helper = binary.parent().unwrap().join("workspace-host");
            let service = Service::start(home, helper)?;
            if let Some(path) = demo_repository {
                let snapshot = answer(&service, ask::snapshot())?;
                // A store without a saved setup starts nothing, the demo project included.
                if snapshot.projects.is_empty() && snapshot.model_selection.is_saved() {
                    let project = answer(&service, ask::create_project("Workspace trial".into()))?;
                    let base = "HEAD".into();
                    answer(
                        &service,
                        ask::attach_repository(Some(project.id), path, base),
                    )?;
                }
            }
            Ok(service)
        })();
        Self {
            service: result.map_err(|e| format!("{e:#}")),
        }
    }
    pub(crate) fn request(
        &self,
        command: Command,
    ) -> Result<Receiver<Result<Value, String>>, String> {
        self.service
            .as_ref()
            .map_err(Clone::clone)?
            .request(command)
    }
    /// Sends `command`; its reply arrives when the host answers, or says the host is gone.
    pub(crate) fn send(
        &self,
        command: Command,
    ) -> Result<impl Future<Output = Result<Value, String>> + use<>, String> {
        let receiver = self.request(command)?;
        Ok(
            async move { (receiver.recv().await).unwrap_or_else(|_| Err("Host disconnected".into())) },
        )
    }
    /// Sends `ask`; its reply arrives decoded as what the host answers it with.
    pub(crate) fn ask<R: DeserializeOwned>(
        &self,
        ask: Ask<R>,
    ) -> Result<impl Future<Output = Result<R, String>> + use<R>, String> {
        let reply = self.send(ask.command().clone())?;
        Ok(async move { reply.await.and_then(|reply| ask.decode(reply)) })
    }
    /// Stops running turns and the host; returns once their provider processes are stopped.
    pub(crate) fn shutdown(&self) {
        if let Ok(service) = &self.service {
            service.shutdown();
        }
    }
    pub(crate) fn changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::changes)
    }
    pub(crate) fn step_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::step_changes)
    }
    pub(crate) fn progress_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::progress_changes)
    }
    pub(crate) fn usage_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::usage_changes)
    }
}
