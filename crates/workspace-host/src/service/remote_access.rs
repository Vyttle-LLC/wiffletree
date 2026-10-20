//! The actor's side of remote access: the host's own remote-access commands, and what paired
//! devices' connections send, checked against each device's level in the store.
use super::{Actor, Event, Reply};
use crate::host_files::{self, Reader};
use crate::remote::{ToActor, access};
use crate::*;

impl Actor {
    /// Runs a remote-access command from the host itself.
    pub(super) fn remote_access(&mut self, command: &Command) -> Result<Value> {
        match command {
            Command::RemoteAccess => {}
            Command::SetRemoteAccess { enabled } => {
                let mut settings = self.host.settings();
                settings.remote_access = *enabled;
                self.host.save_settings(&settings)?;
                self.remote.apply(*enabled);
            }
            Command::OpenPairing => self.remote.open_pairing()?,
            Command::CancelPairing => self.remote.close_pairing(),
            Command::SetDeviceLevel { device_id, level } => {
                self.host.set_device_level(device_id, *level)?;
                self.remote.close_device(device_id);
            }
            Command::RemoveDevice { device_id } => {
                self.host.remove_device(device_id)?;
                self.remote.close_device(device_id);
            }
            _ => bail!("Not a remote-access command"),
        }
        Ok(serde_json::to_value(self.remote_state()?)?)
    }

    pub(super) fn remote_state(&self) -> Result<RemoteAccess> {
        let connected = self.remote.connected();
        let devices = self.host.devices()?.into_iter().map(|device| PairedDevice {
            connected: connected.contains(&device.id),
            id: device.id,
            name: device.name,
            level: device.level,
            paired_at: device.paired_at,
            last_connected_at: device.last_connected_at,
        });
        let enabled = self.host.settings().remote_access;
        Ok(RemoteAccess {
            enabled,
            host_name: self.remote.name().into(),
            host_id: self.remote.host_id(),
            addresses: if enabled {
                crate::remote::addresses()
            } else {
                vec![]
            },
            port: self.remote.port(),
            error: self.remote.error(),
            pairing: self.remote.pairing(),
            devices: devices.collect(),
        })
    }

    /// Handles what a device's connection sent. Returns the command to run as the host's own
    /// once the device's level allows it; everything else is answered here.
    pub(super) fn remote_message(&mut self, message: ToActor) -> Option<Event> {
        match message {
            ToActor::Command {
                device,
                command,
                reply,
            } => self.device_command(&device, command, reply),
            ToActor::Authenticate {
                device,
                signed,
                signature,
                reply,
            } => {
                let level = self.host.device(&device).ok().flatten().and_then(|stored| {
                    pairing::verify(&stored.public_key, &signed, &signature).then_some(stored.level)
                });
                if level.is_some() {
                    let _ = self.host.device_connected(&device, now());
                    self.state_changed();
                }
                let _ = reply.try_send(level);
                None
            }
            ToActor::Paired { key, name, reply } => {
                let stored = self.host.add_device(&key, &name, now());
                self.remote.close_pairing();
                self.state_changed();
                let _ = reply.try_send(stored.map(|_| ()).map_err(|e| format!("{e:#}")));
                None
            }
            ToActor::Changed => {
                self.remote.refresh();
                self.state_changed();
                None
            }
        }
    }

    fn device_command(&mut self, device: &str, command: Command, reply: Reply) -> Option<Event> {
        let level = match self.host.device(device) {
            Ok(Some(stored)) => stored.level,
            Ok(None) => {
                // Removed while connected: its connections close now if they have not.
                self.remote.close_device(device);
                let _ = reply.try_send(Err(UNPAIRED.into()));
                return None;
            }
            Err(error) => {
                let _ = reply.try_send(Err(format!("{error:#}")));
                return None;
            }
        };
        if let Err(refusal) = access::check(&command, level, self.remote.name()) {
            let _ = reply.try_send(Err(refusal));
            return None;
        }
        let reader = Reader::Device(level);
        match command {
            Command::ResolvePaths { references, bases } => {
                self.files_off_thread(reply, move |roots| {
                    let home = std::env::var_os("HOME").map(PathBuf::from);
                    let resolved = host_files::resolve_paths_within(
                        &roots,
                        &references,
                        &bases,
                        home.as_deref(),
                    );
                    Ok(serde_json::to_value(resolved)?)
                });
                None
            }
            Command::ListFolder { path } => {
                self.files_off_thread(reply, move |roots| {
                    Ok(serde_json::to_value(host_files::list_folder(
                        &roots, &path, reader,
                    )?)?)
                });
                None
            }
            command => Some(Event::Command(command, reply)),
        }
    }

    /// Runs `read` against the host's roots on its own thread.
    fn files_off_thread(
        &self,
        reply: Reply,
        read: impl FnOnce(host_files::Roots) -> Result<Value> + Send + 'static,
    ) {
        match self.host.file_roots() {
            Ok(roots) => {
                std::thread::spawn(move || {
                    let _ = reply.try_send(read(roots).map_err(|e| format!("{e:#}")));
                });
            }
            Err(error) => {
                let _ = reply.try_send(Err(format!("{error:#}")));
            }
        }
    }
}
