use async_channel::Receiver;
use serde_json::Value;
use std::path::PathBuf;
use workspace_core::*;
use workspace_host::service::Service;

#[derive(Clone)]
pub struct Bridge {
    service: Result<Service, String>,
}
impl Bridge {
    pub fn start(home: PathBuf, demo_repository: Option<String>) -> Self {
        let result = (|| -> anyhow::Result<Service> {
            let binary = std::env::current_exe()?;
            let helper = binary.parent().unwrap().join("workspace-host");
            let service = Service::start(home, helper)?;
            if let Some(path) = demo_repository {
                let snapshot: Snapshot = serde_json::from_value(
                    service
                        .request(Command::Snapshot)
                        .map_err(anyhow::Error::msg)?
                        .recv_blocking()?
                        .map_err(anyhow::Error::msg)?,
                )?;
                // A store without a saved setup starts nothing, the demo project included.
                if snapshot.projects.is_empty() && snapshot.model_selection.is_saved() {
                    let project: Project = serde_json::from_value(
                        service
                            .request(Command::CreateProject {
                                name: "Workspace trial".into(),
                            })
                            .map_err(anyhow::Error::msg)?
                            .recv_blocking()?
                            .map_err(anyhow::Error::msg)?,
                    )?;
                    service
                        .request(Command::AttachRepository {
                            project_id: Some(project.id),
                            path,
                            base: "HEAD".into(),
                        })
                        .map_err(anyhow::Error::msg)?
                        .recv_blocking()?
                        .map_err(anyhow::Error::msg)?;
                }
            }
            Ok(service)
        })();
        Self {
            service: result.map_err(|e| format!("{e:#}")),
        }
    }
    pub fn request(&self, command: Command) -> Result<Receiver<Result<Value, String>>, String> {
        self.service
            .as_ref()
            .map_err(Clone::clone)?
            .request(command)
    }
    /// Stops running turns and the host; returns once their provider processes are stopped.
    pub fn shutdown(&self) {
        if let Ok(service) = &self.service {
            service.shutdown();
        }
    }
    pub fn changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::changes)
    }
    pub fn step_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::step_changes)
    }
    pub fn progress_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::progress_changes)
    }
    pub fn usage_changes(&self) -> Option<Receiver<()>> {
        self.service.as_ref().ok().map(Service::usage_changes)
    }
}
