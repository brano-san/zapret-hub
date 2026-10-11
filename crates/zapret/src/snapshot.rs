//! What a mutating operation may change, written down before it starts (spec 10):
//! `data/state/pending.json` outlives a crash, `restore` puts everything back.

use std::path::{Path, PathBuf};

use crate::folder::Folder;
use crate::game_filter::{self, GameFilter};
use crate::ipset::{self, IpsetMode};
use crate::procs::{self, Proc};
use crate::service::{self, SERVICE, SERVICE2, SvcState, WINWS2};

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
    /// The same three for zapret2: the `zapret2` service, its `zapret2:<name>` and winws2 started
    /// by hand (zapret2 spec 6). A `pending.json` from before zapret2 has none of them.
    #[serde(default)]
    pub service2: Option<ServiceSnap>,
    #[serde(default)]
    pub strategy2: Option<String>,
    #[serde(default)]
    pub manual_winws2: Vec<Proc>,
    /// Whether the snapshot recorded the zapret2 engine: false only in a `pending.json` from before
    /// zapret2, whose restore leaves zapret2 alone.
    #[serde(default)]
    pub zapret2: bool,
    /// The Flowseal files below: `None` (and `false`) when no Flowseal folder was set, as when
    /// zapret2 runs alone.
    pub ipset: Option<IpsetMode>,
    pub ipset_backup: bool,
    /// The flag file `utils\game_filter.enabled`, which the `game_filter` operation writes
    /// before it reinstalls the service.
    pub game_filter: Option<GameFilter>,
}

/// `data/state/pending.json`
pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state").join("pending.json")
}

/// Starts a mutating operation (spec 10). A `pending` file an earlier operation left is restored
/// first; when that fails, or the file cannot be read, nothing starts and the file stays. After a
/// successful restore the file is removed; then the state is taken and written to `pending`.
pub fn begin(folder: Option<&Folder>, operation: &str, pending: &Path) -> anyhow::Result<Snapshot> {
    let file = pending.display();
    let earlier = Snapshot::load(pending).map_err(|err| {
        anyhow::anyhow!(
            "{file} cannot be read, so \"{operation}\" did not start. Deleting {file} lets operations go on without restoring the state it holds.\n\n{err:#}"
        )
    })?;
    if let Some(earlier) = earlier {
        let was = &earlier.operation;
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
    /// Without a Flowseal `folder` only the services and processes are recorded.
    pub fn take(folder: Option<&Folder>, operation: &str) -> anyhow::Result<Self> {
        let (service, strategy, manual_winws) = engine(SERVICE, "winws.exe", operation)?;
        let (service2, strategy2, manual_winws2) = engine(SERVICE2, WINWS2, operation)?;
        Ok(Self {
            operation: operation.to_owned(),
            service,
            strategy,
            manual_winws,
            service2,
            strategy2,
            manual_winws2,
            zapret2: true,
            ipset: folder.map(ipset::try_mode).transpose()?,
            ipset_backup: folder.is_some_and(ipset::has_backup),
            game_filter: folder.map(game_filter::try_read).transpose()?,
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
            Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
        }
    }

    pub fn clear(path: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(anyhow::Error::new(err).context(path.display().to_string()))
            }
            _ => Ok(()),
        }
    }

    /// The files: the DPI test switch undone, the ipset mode and the game filter as they were. The
    /// game filter is written only when it reads differently, so a file edited by hand stays.
    /// Nothing to do when the snapshot recorded no Flowseal files; an error when it did and there
    /// is no folder to put them back in.
    pub fn restore_files(&self, folder: Option<&Folder>) -> anyhow::Result<()> {
        let (Some(mode), Some(filter)) = (self.ipset, &self.game_filter) else {
            return Ok(());
        };
        let folder = folder.ok_or_else(|| {
            anyhow::anyhow!(
                "the ipset mode and the game filter cannot be put back: {}",
                crate::folder::NOT_SET
            )
        })?;
        ipset::leave_test_any(folder)?;
        ipset::set_mode(folder, mode)?;
        if game_filter::read(folder) != *filter {
            game_filter::write(folder, filter)?;
        }
        Ok(())
    }

    /// Everything. Every step is tried even after one fails; the failures come back together.
    /// zapret2 is part of it only when the snapshot recorded it.
    pub fn restore(&self, folder: Option<&Folder>) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();
        let mut note = |step: &str, result: anyhow::Result<()>| {
            if let Err(err) = result {
                errors.push(format!("{step}: {err:#}"));
            }
        };
        note("files", self.restore_files(folder));
        let mut engines = vec![(SERVICE, "winws.exe")];
        if self.zapret2 {
            engines.push((SERVICE2, WINWS2));
        }
        for (name, image) in engines {
            note(&format!("stop {name}"), service::stop(name));
            note(&format!("stop {image}"), procs::kill_all(image));
        }
        note(
            "service",
            restore_service(SERVICE, self.service.as_ref(), self.strategy.as_deref()),
        );
        if self.zapret2 {
            note(
                "service zapret2",
                restore_service(SERVICE2, self.service2.as_ref(), self.strategy2.as_deref()),
            );
        }
        let manual2 = self.manual_winws2.iter().filter(|_| self.zapret2);
        for proc in self.manual_winws.iter().chain(manual2) {
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
}

/// One engine's service, its recorded strategy and the processes of its image the service did
/// not start. Fails closed, as `take`.
fn engine(
    name: &str,
    image: &str,
    operation: &str,
) -> anyhow::Result<(Option<ServiceSnap>, Option<String>, Vec<Proc>)> {
    let status = service::status(name);
    let config = service::config(name);
    anyhow::ensure!(
        status.is_none() || config.is_some(),
        "the {name} service exists, but its ImagePath could not be read from the registry, so \"{operation}\" did not start; remove the service with `sc delete {name}` if it is broken"
    );
    let snap = config.map(|config| ServiceSnap {
        image_path: config.image_path,
        auto_start: config.auto_start,
        running: matches!(
            status,
            Some((SvcState::Running | SvcState::StartPending, _))
        ),
    });
    let service_pid = status.map(|(_, pid)| pid).filter(|pid| *pid != 0);
    let manual = procs::find(image)
        .into_iter()
        .filter(|p| Some(p.pid) != service_pid)
        .collect();
    Ok((snap, service::strategy_of(name), manual))
}

/// One engine's service as the snapshot holds it: gone when there was none, else its
/// configuration, strategy name and state. The name and the state do not depend on each other:
/// both are done, and the first error comes back.
fn restore_service(
    name: &str,
    snap: Option<&ServiceSnap>,
    strategy: Option<&str>,
) -> anyhow::Result<()> {
    let Some(snap) = snap else {
        return service::stop_and_delete(name);
    };
    let same = service::config(name).is_some_and(|config| {
        config.image_path == snap.image_path && config.auto_start == snap.auto_start
    });
    if !same {
        service::stop_and_delete(name)?;
        service::create(name, &snap.image_path, snap.auto_start)?;
    }
    let named = strategy.map_or(Ok(()), |strategy| service::set_strategy_of(name, strategy));
    let state = if snap.running {
        service::start(name)
    } else {
        service::stop(name)
    };
    named.and(state)
}
