// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The systemd D-Bus client, on its own thread (FRN-SRS-012, FRN-SRS-016,
//! FRN-SRS-034, FRN-SRS-050).
//!
//! One thread connects, polls the manager every [`POLL_INTERVAL`] and sends
//! what it reads to the render loop as [`Event`]s. Every call is bounded by
//! [`METHOD_TIMEOUT`], so a wedged PID 1 costs this thread half a second per
//! call and the render loop nothing: the loop never waits on the channel, and
//! the 2 s degrade deadline (FRN-SRS-016) is the loop's own, independent of
//! how long a connect attempt takes (FRN-SRS-037).
//!
//! `Progress` and `SystemState` are annotated `EmitsChangedSignal=false` by
//! systemd, so the proxies never cache properties: a cached value would never
//! be refreshed and the bar would freeze.
//!
//! The thread lives until the process exits; the splash exits by returning
//! from `main`, which ends it.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use zbus::blocking::{Connection, connection};
use zbus::proxy;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::splash::events::{Event, Snapshot};

/// Bound on every method call and property read.
///
/// A healthy systemd answers in well under a millisecond; half a second is
/// generous for a loaded early boot and short enough that a failed unit is
/// still noticed within FRN-SRS-034's second.
pub const METHOD_TIMEOUT: Duration = Duration::from_millis(500);

/// Time between polls.
///
/// Four polls a second detect a failed unit or rescue mode well within the 1 s
/// FRN-SRS-034 allows, and feed the eased bar more often than it moves
/// visibly; each poll is four or five round trips of microseconds.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Pause between connection attempts while the bus is not up yet.
///
/// `dbus.socket` starts early in stage 2; a 100 ms retry finds it within a
/// frame or three of its appearing without spinning.
pub const RETRY_PAUSE: Duration = Duration::from_millis(100);

/// One row of `Manager.ListJobs`, D-Bus signature `(usssoo)`: job id, unit
/// name, job type, job state, job object path, unit object path.
type Job = (
    u32,
    String,
    String,
    String,
    OwnedObjectPath,
    OwnedObjectPath,
);

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1",
    gen_async = false,
    blocking_name = "ManagerProxy"
)]
trait Manager {
    /// Job-queue progress, 0.0 to 1.0.
    #[zbus(property(emits_changed_signal = "false"))]
    fn progress(&self) -> zbus::Result<f64>;

    /// Units in the failed state.
    #[zbus(property, name = "NFailedUnits")]
    fn n_failed_units(&self) -> zbus::Result<u32>;

    /// `initializing`, `starting`, `running`, `degraded`, `maintenance`, `stopping`.
    #[zbus(property(emits_changed_signal = "false"))]
    fn system_state(&self) -> zbus::Result<String>;

    /// The job queue.
    fn list_jobs(&self) -> zbus::Result<Vec<Job>>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1",
    gen_async = false,
    blocking_name = "UnitProxy"
)]
trait Unit {
    /// The unit's description, as `systemctl status` shows it.
    #[zbus(property(emits_changed_signal = "const"))]
    fn description(&self) -> zbus::Result<String>;
}

/// The system bus socket, named outright.
///
/// `Builder::system()` would consult `DBUS_SYSTEM_BUS_ADDRESS`, an environment
/// input read behind the CLI's back; the splash runs as a system unit, where
/// the socket's path is fixed.
pub const SYSTEM_BUS: &str = "unix:path=/run/dbus/system_bus_socket";

/// systemd's private manager socket, which the client refuses.
///
/// zbus 5.19 panics on the replies PID 1 sends over it, and the release
/// profile turns any panic into an abort of the whole splash.
const PRIVATE_SOCKET: &str = "/run/systemd/private";

/// Checks a `--bus` value: `system`, `none`, or a peer-to-peer D-Bus address
/// (tests serve a fake manager this way).
///
/// # Errors
///
/// Why the value cannot be used: not a D-Bus address, or systemd's private
/// socket.
pub fn check_bus(bus: &str) -> Result<(), String> {
    if bus == "system" || bus == "none" {
        return Ok(());
    }
    if bus.contains(PRIVATE_SOCKET) {
        return Err(format!(
            "`{PRIVATE_SOCKET}` is not supported; use the system bus"
        ));
    }
    zbus::Address::try_from(bus)
        .map(drop)
        .map_err(|e| format!("`{bus}` is not a D-Bus address: {e}"))
}

/// Starts the client for `bus` and returns its message channel.
///
/// `bus` is `system` for the system bus, or a peer-to-peer address that
/// [`check_bus`] accepted. If the thread cannot be started the channel simply
/// stays silent, which the loop treats as D-Bus being unavailable
/// (FRN-SRS-016).
#[must_use]
pub fn spawn(bus: &str) -> Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    let bus = bus.to_owned();
    // A failure to spawn drops `tx`; the loop then degrades on its deadline.
    let _ = std::thread::Builder::new()
        .name("fairing-dbus".to_owned())
        .spawn(move || run(&bus, &tx));
    rx
}

/// Connects, polls until the connection fails, and starts over, until the
/// render loop has gone away.
fn run(bus: &str, tx: &Sender<Event>) {
    loop {
        let alive = match connect(bus) {
            Ok(connection) => tx.send(Event::Connected).is_ok() && poll(&connection, tx),
            Err(error) => tx.send(Event::Lost(error.to_string())).is_ok(),
        };
        if !alive {
            return;
        }
        std::thread::sleep(RETRY_PAUSE);
    }
}

fn connect(bus: &str) -> zbus::Result<Connection> {
    let builder = if bus == "system" {
        connection::Builder::address(SYSTEM_BUS)?
    } else {
        connection::Builder::address(bus)?.p2p()
    };
    builder.method_timeout(METHOD_TIMEOUT).build()
}

/// Polls until a call fails (returns `true`: reconnect) or the loop is gone
/// (returns `false`).
fn poll(connection: &Connection, tx: &Sender<Event>) -> bool {
    let manager = match ManagerProxy::builder(connection)
        .cache_properties(CacheProperties::No)
        .build()
    {
        Ok(manager) => manager,
        Err(error) => return tx.send(Event::Lost(error.to_string())).is_ok(),
    };
    loop {
        let event = match snapshot(connection, &manager) {
            Ok(snapshot) => Event::Snapshot(snapshot),
            Err(error) => return tx.send(Event::Lost(error.to_string())).is_ok(),
        };
        if tx.send(event).is_err() {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// One reading of the manager, with the newest job's unit description.
fn snapshot(connection: &Connection, manager: &ManagerProxy<'_>) -> zbus::Result<Snapshot> {
    let progress = manager.progress()?;
    let failed_units = manager.n_failed_units()?;
    let system_state = manager.system_state()?;
    let jobs = manager.list_jobs()?;
    // Job ids only grow, so the highest running id is the job started last.
    let newest = jobs
        .iter()
        .filter(|job| job.3 == "running")
        .max_by_key(|job| job.0)
        .or_else(|| jobs.iter().max_by_key(|job| job.0));
    let job = match newest {
        Some(job) => {
            let unit = UnitProxy::builder(connection)
                .path(job.5.clone())?
                .cache_properties(CacheProperties::No)
                .build()?;
            // A unit can vanish between ListJobs and the read; that is no reading of the job, not a lost bus.
            unit.description().ok().filter(|d| !d.trim().is_empty())
        }
        None => None,
    };
    Ok(Snapshot {
        progress,
        failed_units,
        system_state,
        job,
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    use zbus::{Guid, interface};

    use super::*;

    const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
    const UDEV_PATH: &str = "/org/freedesktop/systemd1/unit/systemd_2dudevd_2eservice";
    const NETWORK_PATH: &str = "/org/freedesktop/systemd1/unit/network_2etarget";

    struct FakeManager {
        progress: f64,
        failed: u32,
        state: &'static str,
    }

    #[interface(name = "org.freedesktop.systemd1.Manager")]
    impl FakeManager {
        #[zbus(property(emits_changed_signal = "false"))]
        fn progress(&self) -> f64 {
            self.progress
        }

        #[zbus(property, name = "NFailedUnits")]
        fn n_failed_units(&self) -> u32 {
            self.failed
        }

        #[zbus(property(emits_changed_signal = "false"))]
        fn system_state(&self) -> String {
            self.state.to_owned()
        }

        #[expect(clippy::unused_self, reason = "zbus interface methods take `&self`")]
        fn list_jobs(&self) -> Vec<Job> {
            let job = |id: u32, state: &str, path: &str| {
                (
                    id,
                    "unit".to_owned(),
                    "start".to_owned(),
                    state.to_owned(),
                    OwnedObjectPath::try_from(format!("/org/freedesktop/systemd1/job/{id}"))
                        .unwrap_or_else(|e| panic!("{e}")),
                    OwnedObjectPath::try_from(path).unwrap_or_else(|e| panic!("{e}")),
                )
            };
            vec![
                job(17, "running", UDEV_PATH),
                job(23, "waiting", NETWORK_PATH),
            ]
        }
    }

    struct FakeUnit(&'static str);

    #[interface(name = "org.freedesktop.systemd1.Unit")]
    impl FakeUnit {
        #[zbus(property(emits_changed_signal = "const"))]
        fn description(&self) -> String {
            self.0.to_owned()
        }
    }

    /// Serves a fake manager on a listening socket in `dir`; returns its address.
    fn serve(dir: &std::path::Path, failed: u32, state: &'static str) -> String {
        let path = dir.join("systemd");
        let listener = UnixListener::bind(&path).unwrap_or_else(|e| panic!("{e}"));
        std::thread::spawn(move || {
            // One peer per test; the connection lives as long as the thread.
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let served = connection::Builder::async_io_unix_stream(stream)
                .p2p()
                .server(Guid::generate())
                .and_then(|b| {
                    b.serve_at(
                        MANAGER_PATH,
                        FakeManager {
                            progress: 0.42,
                            failed,
                            state,
                        },
                    )
                })
                .and_then(|b| {
                    b.serve_at(UDEV_PATH, FakeUnit("Rule-based Manager for Device Events"))
                })
                .and_then(|b| b.serve_at(NETWORK_PATH, FakeUnit("Network")))
                .and_then(connection::Builder::build);
            if let Ok(_connection) = served {
                std::thread::sleep(Duration::from_secs(30));
            }
        });
        format!("unix:path={}", path.display())
    }

    fn first_snapshot(rx: &Receiver<Event>) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Event::Snapshot(snapshot)) = rx.recv_timeout(Duration::from_secs(1)) {
                return snapshot;
            }
        }
        panic!("no snapshot within 10 s");
    }

    #[test]
    fn reads_progress_failures_state_and_the_running_job() {
        // Verifies: FRN-SRS-012, FRN-SRS-050
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let rx = spawn(&serve(dir.path(), 2, "degraded"));
        let snapshot = first_snapshot(&rx);
        assert!((snapshot.progress - 0.42).abs() < f64::EPSILON);
        assert_eq!(snapshot.failed_units, 2);
        assert_eq!(snapshot.system_state, "degraded");
        assert_eq!(
            snapshot.job.as_deref(),
            Some("Rule-based Manager for Device Events"),
            "the running job, not the newer waiting one"
        );
    }

    #[test]
    fn an_absent_bus_is_reported_and_retried() {
        // Verifies: FRN-SRS-016
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let address = format!("unix:path={}", dir.path().join("nobody").display());
        let rx = spawn(&address);
        let started = Instant::now();
        let mut lost = 0;
        while lost < 2 {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Lost(_)) => lost += 1,
                Ok(other) => panic!("unexpected {other:?}"),
                Err(e) => panic!("{e}"),
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn bus_values_are_checked_before_any_thread_starts() {
        for good in ["system", "none", "unix:path=/tmp/fake-manager"] {
            assert_eq!(check_bus(good), Ok(()), "{good}");
        }
        let private = check_bus("unix:path=/run/systemd/private")
            .err()
            .unwrap_or_default();
        assert!(private.contains("not supported"), "{private}");
        assert!(check_bus("not an address").is_err());
    }
}
