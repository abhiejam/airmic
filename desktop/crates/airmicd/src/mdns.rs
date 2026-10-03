use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use uuid::Uuid;

use crate::config;

const SERVICE_TYPE: &str = "_airmic._tcp.local.";

/// A registered mDNS advert.
pub struct Advert {
    daemon: ServiceDaemon,
    fullname: String,
}

/// Advertises this computer as `_airmic._tcp` on the control port.
pub fn advertise(control_port: u16) -> anyhow::Result<Advert> {
    let id = load_or_create_device_id(&config::config_dir()?)?;
    let hostname = gethostname::gethostname().to_string_lossy().into_owned();
    let properties = [("id", id.as_str()), ("name", hostname.as_str()), ("v", "1")];
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        &hostname,
        &format!("{hostname}.local."),
        (),
        control_port,
        &properties[..],
    )?
    .enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    let daemon = ServiceDaemon::new()?;
    daemon.register(info)?;
    Ok(Advert { daemon, fullname })
}

impl Advert {
    /// Sends the mDNS goodbye so phones drop this computer at once.
    pub fn withdraw(self) {
        if let Ok(status) = self.daemon.unregister(&self.fullname) {
            let _ = status.recv_timeout(Duration::from_secs(1));
        }
        let _ = self.daemon.shutdown();
    }
}

/// Returns the computer id stored in `dir/device_id`, creating a new one on first use.
pub fn load_or_create_device_id(dir: &Path) -> anyhow::Result<String> {
    let path = dir.join("device_id");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let id = Uuid::parse_str(text.trim())
                .with_context(|| format!("parsing {}", path.display()))?;
            Ok(id.to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = Uuid::new_v4().to_string();
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::write(&path, format!("{id}\n"))
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(id)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("airmic");
        let first = load_or_create_device_id(&config_dir).unwrap();
        assert_eq!(Uuid::parse_str(&first).unwrap().get_version_num(), 4);
        assert_eq!(load_or_create_device_id(&config_dir).unwrap(), first);
    }
}
