//! What a mutating operation may change, written down before it starts (spec 10):
//! `data/state/pending.json` outlives a crash, `restore` puts everything back.

use std::path::{Path, PathBuf};

use crate::folder::Folder;
use crate::game_filter::{self, GameFilter};
use crate::ipset::{self, IpsetMode};
use crate::procs::{self, Proc};
use crate::service::{self, SERVICE, SvcState};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceSnap {
    pub image_path: String,
    pub auto_start: bool,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// The operation it guards: `apply`, `tests`, `ipset`, `game_filter`, `remove_service`.
    pub operation: String,
    pub service: Option<ServiceSnap>,
    /// `zapret-discord-youtube` in the registry.
    pub strategy: Option<String>,
    /// winws started by a `.bat`, not by the service.
    pub manual_winws: Vec<Proc>,
    pub ipset: IpsetMode,
    pub ipset_backup: bool,
    /// The flag file `utils\game_filter.enabled`, which the `game_filter` operation writes
    /// before it reinstalls the service.
    pub game_filter: GameFilter,
}

/// `data/state/pending.json`
pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state").join("pending.json")
}

/// Starts a mutating operation (spec 10). A `pending` file an earlier operation left is restored
/// first; when that fails, or the file cannot be read, nothing starts and the file stays. After a
/// successful restore the file is removed; then the state is taken and written to `pending`.
pub fn begin(folder: &Folder, operation: &str, pending: &Path) -> anyhow::Result<Snapshot> {
    if let Some(earlier) = Snapshot::load(pending)? {
        let (file, was) = (pending.display(), &earlier.operation);
        earlier.restore(folder).map_err(|err| {
            anyhow::anyhow!(
                "{file} holds the unfinished operation \"{was}\" and its state could not be restored, so \"{operation}\" did not start. Deleting {file} lets operations go on without restoring that state.\n\n{err:#}"
            )
        })?;
        Snapshot::clear(pending).map_err(|err| {
            anyhow::anyhow!(
                "{file} was restored but could not be removed, so \"{operation}\" did not start: {err:#}"
            )
        })?;
        log::info!(
            "{file}: the state before the unfinished \"{was}\" is back, the file is removed"
        );
    }
    let snapshot = Snapshot::take(folder, operation)?;
    snapshot.save(pending)?;
    Ok(snapshot)
}

impl Snapshot {
    /// Fails closed: what it cannot read would be restored as something else, so an operation
    /// does not start on it.
    pub fn take(folder: &Folder, operation: &str) -> anyhow::Result<Self> {
        let status = service::status(SERVICE);
        let config = service::config(SERVICE);
        anyhow::ensure!(
            status.is_none() || config.is_some(),
            "the zapret service exists, but its ImagePath could not be read from the registry, so \"{operation}\" did not start; remove the service with service.bat or `sc delete zapret` if it is broken"
        );
        let service = config.map(|config| ServiceSnap {
            image_path: config.image_path,
            auto_start: config.auto_start,
            running: matches!(
                status,
                Some((SvcState::Running | SvcState::StartPending, _))
            ),
        });
        let service_pid = status.map(|(_, pid)| pid).filter(|pid| *pid != 0);
        Ok(Self {
            operation: operation.to_owned(),
            service,
            strategy: service::strategy(),
            manual_winws: procs::find("winws.exe")
                .into_iter()
                .filter(|p| Some(p.pid) != service_pid)
                .collect(),
            ipset: ipset::try_mode(folder)?,
            ipset_backup: ipset::has_backup(folder),
            game_filter: game_filter::try_read(folder)?,
        })
    }

    /// Flushed to disk: a power cut right after `begin` must still find the whole file. Written in
    /// place: `begin` saves only after an earlier file was restored and removed, and a write torn
    /// by a crash happens before the operation's first change.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        use std::io::Write as _;
        let json = serde_json::to_string_pretty(self)?;
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut file = std::fs::File::create(path)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()
        };
        write().map_err(|err| anyhow::Error::new(err).context(path.display().to_string()))
    }

    /// `None` when no operation was left unfinished.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(
                serde_json::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?,
            )),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn clear(path: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        }
    }

    /// The files: the DPI test switch undone, the ipset mode and the game filter as they were. The
    /// game filter is written only when it reads differently, so a file edited by hand stays.
    pub fn restore_files(&self, folder: &Folder) -> anyhow::Result<()> {
        ipset::leave_test_any(folder)?;
        ipset::set_mode(folder, self.ipset)?;
        if game_filter::read(folder) != self.game_filter {
            game_filter::write(folder, &self.game_filter)?;
        }
        Ok(())
    }

    /// Everything. Every step is tried even after one fails; the failures come back together.
    pub fn restore(&self, folder: &Folder) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();
        let mut note = |step: &str, result: anyhow::Result<()>| {
            if let Err(err) = result {
                errors.push(format!("{step}: {err:#}"));
            }
        };
        note("files", self.restore_files(folder));
        note("stop zapret", service::stop(SERVICE));
        note("stop winws", procs::kill_all("winws.exe"));
        note("service", self.restore_service());
        for proc in &self.manual_winws {
            note("start winws", procs::spawn_detached(proc));
        }
        log::info!(
            "state before {} restored, {} errors",
            self.operation,
            errors.len()
        );
        anyhow::ensure!(errors.is_empty(), "{}", errors.join("\n\n"));
        Ok(())
    }

    fn restore_service(&self) -> anyhow::Result<()> {
        let Some(snap) = &self.service else {
            return service::stop_and_delete(SERVICE);
        };
        let same = service::config(SERVICE).is_some_and(|config| {
            config.image_path == snap.image_path && config.auto_start == snap.auto_start
        });
        if !same {
            service::stop_and_delete(SERVICE)?;
            service::create(&snap.image_path, snap.auto_start)?;
        }
        if let Some(name) = &self.strategy {
            service::set_strategy(name)?;
        }
        if snap.running {
            service::start(SERVICE)
        } else {
            service::stop(SERVICE)
        }
    }
}
