//! What a mutating operation may change, written down before it starts (spec 10):
//! `data/state/pending.json` outlives a crash, `restore` puts everything back.

use std::path::{Path, PathBuf};

use crate::procs::{self, Proc};
use crate::service::{self, SERVICE2, SvcState, WINWS2};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceSnap {
    pub image_path: String,
    pub auto_start: bool,
    pub running: bool,
}

/// The router as it ran: the app puts it back (`zapret` knows nothing of the router).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RouterSnap {
    pub enabled: bool,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// The operation it guards: `apply`, `tests`, `remove_service`. A `pending.json` of 0.8.0
    /// also holds Flowseal's service, winws and files: those fields are not read any more.
    pub operation: String,
    /// The `zapret2` service, its `zapret2:<name>` and winws2 started by hand (zapret2 spec 6). A
    /// `pending.json` from before zapret2 has none of them.
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
    /// Filled by the app; a `pending.json` from before it has none.
    #[serde(default)]
    pub router: Option<RouterSnap>,
    /// Linux: the nftables table file the `zapret2` unit applies (`service::NFT_CONFIG`), as it
    /// was: its ports belong to the recorded `ExecStart`. `None` without one, and on Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nft: Option<String>,
    /// Linux: bol-van/zapret's own `zapret` unit ran. A test run stops it; the restore starts it
    /// again (the user's, never removed by the hub). Always false on Windows, where Flowseal's
    /// `zapret` is a conflict and stays stopped.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub foreign_running: bool,
}

/// `data/state/pending.json`
pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state").join("pending.json")
}

/// Starts a mutating operation (spec 10). A `pending` file an earlier operation left is restored
/// first; when that fails, or the file cannot be read, nothing starts and the file stays. After a
/// successful restore the file is removed; then the state is taken and written to `pending`.
pub fn begin(operation: &str, pending: &Path) -> anyhow::Result<Snapshot> {
    let file = pending.display();
    let earlier = Snapshot::load(pending).map_err(|err| {
        anyhow::anyhow!(
            "{file} cannot be read, so \"{operation}\" did not start. Deleting {file} lets operations go on without restoring the state it holds.\n\n{err:#}"
        )
    })?;
    if let Some(earlier) = earlier {
        let was = &earlier.operation;
        earlier.restore().map_err(|err| {
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
    let snapshot = Snapshot::take(operation)?;
    snapshot.save(pending)?;
    Ok(snapshot)
}

impl Snapshot {
    /// Fails closed: what it cannot read would be restored as something else, so an operation
    /// does not start on it.
    pub fn take(operation: &str) -> anyhow::Result<Self> {
        let (service2, strategy2, manual_winws2) = engine(SERVICE2, WINWS2, operation)?;
        Ok(Self {
            operation: operation.to_owned(),
            service2,
            strategy2,
            manual_winws2,
            zapret2: true,
            router: None,
            #[cfg(windows)]
            nft: None,
            #[cfg(target_os = "linux")]
            nft: std::fs::read_to_string(service::NFT_CONFIG).ok(),
            foreign_running: cfg!(target_os = "linux")
                && matches!(
                    service::state(service::SERVICE),
                    SvcState::Running | SvcState::StartPending
                ),
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

    /// Everything. Every step is tried even after one fails; the failures come back together.
    /// Nothing to do for a snapshot that did not record zapret2.
    pub fn restore(&self) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();
        let mut note = |step: &str, result: anyhow::Result<()>| {
            if let Err(err) = result {
                errors.push(format!("{step}: {err:#}"));
            }
        };
        if self.zapret2 {
            #[cfg(windows)]
            {
                note(&format!("stop {SERVICE2}"), service::stop(SERVICE2));
                note(&format!("stop {WINWS2}"), procs::kill_all(WINWS2));
                note(
                    "service zapret2",
                    restore_service(SERVICE2, self.service2.as_ref(), self.strategy2.as_deref()),
                );
            }
            #[cfg(target_os = "linux")]
            {
                note("service zapret2", self.restore_linux());
                if let Some(strategy) = &self.strategy2 {
                    note("strategy", service::set_strategy_of(SERVICE2, strategy));
                }
            }
            for proc in &self.manual_winws2 {
                note("start winws2", procs::spawn_detached(proc));
            }
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

#[cfg(target_os = "linux")]
impl Snapshot {
    /// The `zapret2` unit, its table file and its state in one root batch (one sudo/pkexec): the
    /// service stopped, every nfqws2 ended, then the unit and table written back (or removed).
    fn restore_linux(&self) -> anyhow::Result<()> {
        let mut batch = crate::root::Batch::new();
        self.add_restore(&mut batch)?;
        batch.exec()
    }

    pub(crate) fn add_restore(&self, batch: &mut crate::root::Batch) -> anyhow::Result<()> {
        batch.try_run(&["systemctl", "stop", "zapret2.service"]);
        batch.try_run(&["pkill", "-KILL", "-x", WINWS2]);
        match &self.nft {
            Some(table) => {
                batch.write(Path::new(service::NFT_CONFIG), table)?;
            }
            None => {
                batch.run(&["rm", "-f", "--", service::NFT_CONFIG]);
            }
        }
        match &self.service2 {
            Some(snap) => service::add_replace(
                batch,
                SERVICE2,
                &snap.image_path,
                snap.auto_start,
                snap.running,
            )?,
            None => service::add_remove(batch, SERVICE2),
        }
        if self.foreign_running {
            batch.run(&["systemctl", "start", "zapret.service"]);
        }
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
        "the {name} service exists, but {}, so \"{operation}\" did not start; {}",
        if cfg!(windows) {
            "its ImagePath could not be read from the registry"
        } else {
            "its ExecStart could not be read (systemctl cat)"
        },
        if cfg!(windows) {
            format!("remove the service with `sc delete {name}` if it is broken")
        } else {
            format!("fix or remove the unit {name}.service if it is broken")
        }
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

#[cfg(windows)]
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_linux_restore_is_one_batch_with_the_unit_and_its_table() {
        let snap = Snapshot {
            operation: "apply".into(),
            service2: Some(ServiceSnap {
                image_path: r#""/z/nfqws2" "--qnum=200""#.into(),
                auto_start: true,
                running: true,
            }),
            strategy2: Some("zapret2:general".into()),
            manual_winws2: Vec::new(),
            zapret2: true,
            router: None,
            nft: Some(
                "table inet zapret_hub {}
"
                .into(),
            ),
            foreign_running: false,
        };
        let mut batch = crate::root::Batch::new();
        snap.add_restore(&mut batch).unwrap();
        let script = batch.script();
        let table = script.find(&format!("'{}'", service::NFT_CONFIG)).unwrap();
        let unit = script
            .find("'/etc/systemd/system/zapret2.service'")
            .unwrap();
        let restart = script
            .find("'systemctl' 'restart' 'zapret2.service'")
            .unwrap();
        assert!(table < unit && unit < restart, "{script}");

        let gone = Snapshot {
            service2: None,
            nft: None,
            ..snap
        };
        let mut batch = crate::root::Batch::new();
        gone.add_restore(&mut batch).unwrap();
        let script = batch.script();
        assert!(
            script.contains("'rm' '-f' '--' '/etc/zapret-hub/nfqws2.nft'"),
            "{script}"
        );
        assert!(
            script.contains("'rm' '-f' '--' '/etc/systemd/system/zapret2.service'"),
            "{script}"
        );
        assert!(!script.contains("'zapret.service'"), "{script}");

        // bol-van's `zapret` ran before a test run stopped it: it runs again, last.
        let foreign = Snapshot {
            foreign_running: true,
            ..gone
        };
        let mut batch = crate::root::Batch::new();
        foreign.add_restore(&mut batch).unwrap();
        assert!(
            batch
                .script()
                .ends_with("'systemctl' 'start' 'zapret.service'\n"),
            "{}",
            batch.script()
        );
    }
}
