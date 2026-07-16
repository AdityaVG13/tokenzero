//! Machine-wide CodeMode permit (slot layout, reclaim, backoff).
//!
//! Shared contract for TokenZero / FSZero / GraphZero: directory-based locks
//! under `/tmp/zerostack-codemode-*.permit` with `slot-N` children. Live holders
//! block peers until wall deadline (retryable busy); dead / incomplete dirs are
//! reclaimed. Fatal I/O (EACCES, etc.) stays non-retryable.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const PERMIT_POLL: Duration = Duration::from_millis(20);
pub(crate) const PERMIT_POLL_MAX: Duration = Duration::from_millis(200);
const INCOMPLETE_PERMIT_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) struct MachinePermit(pub(crate) PathBuf, String);

impl MachinePermit {
    pub(crate) fn acquire_slots(
        base: &Path,
        slots: usize,
        deadline: Instant,
        command: &str,
    ) -> Result<Self, AcquireError> {
        // Always use base/slot-N — even when slots==1 — so mixed concurrency
        // envs cannot stack an exclusive base lock with slot children.
        // Pool size is the caller's requested budget (from env); do not freeze
        // capacity to the first asker — that would let CONCURRENCY=1 starve the
        // family-wide cores/4 analysis budget.
        let slots = slots.max(1);
        let mut attempt = 0u32;
        loop {
            if !legacy_exclusive_busy(base) {
                let _ = fs::create_dir_all(base);
                for idx in 0..slots {
                    let path = base.join(format!("slot-{idx}"));
                    match Self::try_create(&path, command) {
                        Ok(permit) => return Ok(permit),
                        Err(TryPermit::Busy) => {}
                        Err(TryPermit::Fatal(e)) => return Err(AcquireError::Fatal(e)),
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(AcquireError::Busy(format!(
                    "codemode permit {} is held by live process(es) across {slots} slots",
                    base.display()
                )));
            }
            // Back off under multi-waiter pressure so 100 idle sessions do not
            // wake-storm the slot directory every 20ms.
            let sleep_for = permit_backoff(attempt)
                .min(deadline.saturating_duration_since(Instant::now()));
            attempt = attempt.saturating_add(1);
            std::thread::sleep(sleep_for);
        }
    }

    /// Legacy exclusive single-dir permit (pre-slot layout). Production paths
    /// use `acquire_slots`; this remains for reclaim interop tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn acquire(
        path: &Path,
        deadline: Instant,
        command: &str,
    ) -> Result<Self, AcquireError> {
        let mut attempt = 0u32;
        loop {
            match Self::try_create(path, command) {
                Ok(permit) => return Ok(permit),
                Err(TryPermit::Busy) => {
                    if Instant::now() >= deadline {
                        return Err(AcquireError::Busy(format!(
                            "codemode permit {} is held by a live process",
                            path.display()
                        )));
                    }
                    let sleep_for = permit_backoff(attempt)
                        .min(deadline.saturating_duration_since(Instant::now()));
                    attempt = attempt.saturating_add(1);
                    std::thread::sleep(sleep_for);
                }
                Err(TryPermit::Fatal(e)) => return Err(AcquireError::Fatal(e)),
            }
        }
    }

    fn try_create(path: &Path, command: &str) -> Result<Self, TryPermit> {
        match fs::create_dir(path) {
            Ok(()) => {
                let owner = format!(
                    "{}-{}-{:?}",
                    std::process::id(),
                    epoch_millis(),
                    std::thread::current().id()
                );
                if let Err(e) = write_metadata(path, &owner, command) {
                    cleanup_owned(path, &owner);
                    return Err(TryPermit::Fatal(format!(
                        "write codemode permit metadata: {e}"
                    )));
                }
                Ok(Self(path.to_path_buf(), owner))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if reclaim_dead(path) {
                    return Self::try_create(path, command);
                }
                Err(TryPermit::Busy)
            }
            Err(e) => Err(TryPermit::Fatal(format!(
                "create codemode permit {}: {e}",
                path.display()
            ))),
        }
    }
}

enum TryPermit {
    Busy,
    Fatal(String),
}

#[derive(Debug)]
pub(crate) enum AcquireError {
    /// Live holder(s) still hold the permit after the wall deadline.
    Busy(String),
    /// Non-retryable I/O / policy failure creating the permit (EACCES, etc.).
    Fatal(String),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(message) | Self::Fatal(message) => f.write_str(message),
        }
    }
}

impl Drop for MachinePermit {
    fn drop(&mut self) {
        cleanup_owned(&self.0, &self.1)
    }
}

fn write_metadata(path: &Path, owner: &str, command: &str) -> std::io::Result<()> {
    // Write ownership first so an error in any later metadata write remains
    // removable by the acquiring RAII guard.
    fs::write(path.join("owner"), owner)?;
    fs::write(path.join("pid"), std::process::id().to_string())?;
    fs::write(
        path.join("repository"),
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .chars()
            .take(1024)
            .collect::<String>(),
    )?;
    fs::write(path.join("command"), command)?;
    fs::write(path.join("started_at"), epoch_millis().to_string())
}

const PERMIT_METADATA: &[&str] = &["pid", "repository", "command", "started_at", "owner"];

fn remove_permit(path: &Path) -> bool {
    for name in PERMIT_METADATA {
        let _ = fs::remove_file(path.join(name));
    }
    fs::remove_dir(path).is_ok()
}

fn cleanup_owned(path: &Path, owner: &str) {
    if fs::read_to_string(path.join("owner")).ok().as_deref() == Some(owner) {
        remove_permit(path);
    }
}

fn reclaim_dead(path: &Path) -> bool {
    let pid = fs::read_to_string(path.join("pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok());
    if let Some(pid) = pid {
        return !process_alive(pid) && remove_permit(path);
    }

    // A process can die after create_dir() but before writing pid. Without a
    // bounded incomplete-state recovery, that empty permit blocks every
    // CodeMode client forever. The grace period avoids racing a live writer.
    let stale = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= INCOMPLETE_PERMIT_GRACE);
    stale && remove_permit(path)
}

/// Legacy exclusive layout put `pid`/`owner` directly under `base`. Slot layout
/// keeps metadata only under `base/slot-N`.
fn looks_like_legacy_exclusive_permit(base: &Path) -> bool {
    base.is_dir() && (base.join("pid").is_file() || base.join("owner").is_file())
}

/// If `base` is a live legacy exclusive permit, reclaim dead holders; otherwise
/// treat every slot as Busy so peers cannot create `slot-N` children underneath.
fn legacy_exclusive_busy(base: &Path) -> bool {
    if !looks_like_legacy_exclusive_permit(base) {
        return false;
    }
    let _ = reclaim_dead(base);
    looks_like_legacy_exclusive_permit(base)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(unix))]
fn process_alive(_: u32) -> bool {
    true
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_millis())
}

pub(crate) fn permit_backoff(attempt: u32) -> Duration {
    // 20, 40, 80, 160, 200, 200, ...
    let shift = attempt.min(4);
    let millis = (PERMIT_POLL.as_millis() as u64)
        .saturating_mul(1u64 << shift)
        .min(PERMIT_POLL_MAX.as_millis() as u64)
        .max(PERMIT_POLL.as_millis() as u64);
    Duration::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn reclaims_incomplete_machine_permit_after_grace() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-incomplete-permit-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("owner"), "").unwrap();
        std::thread::sleep(INCOMPLETE_PERMIT_GRACE + Duration::from_millis(20));

        assert!(reclaim_dead(&path));
        assert!(!path.exists());
    }

    #[test]
    fn analysis_permit_is_exclusive_across_threads() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-analysis-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        let barrier = Arc::new(Barrier::new(2));
        let path_holder = path.clone();
        let barrier_holder = Arc::clone(&barrier);
        let holder = thread::spawn(move || {
            let permit = MachinePermit::acquire(
                &path_holder,
                Instant::now() + Duration::from_secs(5),
                "test-analysis-holder",
            )
            .expect("holder acquires analysis permit");
            barrier_holder.wait();
            thread::sleep(Duration::from_millis(300));
            drop(permit);
        });

        barrier.wait();
        let contested = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_millis(80),
            "test-analysis-contender",
        );
        assert!(
            contested.is_err(),
            "second acquirer must not stack while holder is live: {contested:?}"
        );
        holder.join().unwrap();
        let after = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_secs(2),
            "test-analysis-after",
        );
        assert!(after.is_ok(), "permit must release for the next waiter");
    }

    #[test]
    fn multi_slot_analysis_permit_allows_parallel_holders() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-analysis-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let a = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "slot-a",
        )
        .expect("first slot");
        let b = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "slot-b",
        )
        .expect("second slot");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "slot-c",
        );
        assert!(
            contested.is_err(),
            "third holder must wait when only two slots exist"
        );
        drop(a);
        drop(b);
    }

    #[test]
    fn index_permit_is_exclusive_across_threads() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-index-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        let barrier = Arc::new(Barrier::new(2));
        let path_holder = path.clone();
        let barrier_holder = Arc::clone(&barrier);
        let holder = thread::spawn(move || {
            let permit = MachinePermit::acquire(
                &path_holder,
                Instant::now() + Duration::from_secs(5),
                "test-index-holder",
            )
            .expect("holder acquires index permit");
            barrier_holder.wait();
            thread::sleep(Duration::from_millis(300));
            drop(permit);
        });

        barrier.wait();
        let contested = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_millis(80),
            "test-index-contender",
        );
        assert!(
            contested.is_err(),
            "second acquirer must not stack while holder is live: {contested:?}"
        );
        holder.join().unwrap();
        let after = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_secs(2),
            "test-index-after",
        );
        assert!(after.is_ok(), "permit must release for the next waiter");
    }

    #[test]
    fn multi_slot_index_permit_allows_parallel_holders() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-index-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let a = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "index-slot-a",
        )
        .expect("first index slot");
        let b = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "index-slot-b",
        )
        .expect("second index slot");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "index-slot-c",
        );
        assert!(
            contested.is_err(),
            "third index holder must wait when only two slots exist"
        );
        drop(a);
        drop(b);
    }

    #[test]
    fn permit_backoff_grows_then_caps() {
        assert_eq!(permit_backoff(0), PERMIT_POLL);
        assert!(permit_backoff(3) > permit_backoff(0));
        assert_eq!(permit_backoff(10), PERMIT_POLL_MAX);
    }

    #[test]
    fn slots_one_uses_slot_zero_not_base() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-slot0-layout-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let permit = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_secs(2),
            "slot-one",
        )
        .expect("slots=1 must acquire");
        assert!(
            base.join("slot-0").join("pid").is_file(),
            "slots=1 must lock base/slot-0, not base itself"
        );
        assert!(
            !base.join("pid").is_file(),
            "slots=1 must not write pid directly under base"
        );
        drop(permit);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn mixed_concurrency_layouts_share_slot_namespace() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-mixed-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);

        // slots=1 holds slot-0; slots>1 peer must take another slot child (shared
        // namespace), not invent a nested lock under an exclusive base.
        let holder = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-1",
        )
        .expect("slots=1 holder");
        let peer = MachinePermit::acquire_slots(
            &base,
            3,
            Instant::now() + Duration::from_secs(2),
            "peer-slots-3",
        )
        .expect("slots>1 peer must share slot namespace with slots=1 holder");
        assert_eq!(peer.0.parent(), Some(base.as_path()));
        let peer_name = peer.0.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(
            peer_name.starts_with("slot-") && peer_name != "slot-0",
            "peer must occupy a free slot child, got {}",
            peer.0.display()
        );
        drop(peer);
        drop(holder);

        // Saturated multi-slot pool must reject a slots=1 peer (no stacking past budget).
        let holder = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-2",
        )
        .expect("slots=2 holder");
        let holder2 = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-2b",
        )
        .expect("second slots=2 holder");
        let contested = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_millis(80),
            "peer-slots-1",
        );
        assert!(
            contested.is_err(),
            "slots=1 peer must not stack when multi-slot pool is full: {contested:?}"
        );
        drop(holder);
        drop(holder2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn live_legacy_exclusive_base_blocks_all_slots() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-legacy-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let legacy = MachinePermit::acquire(
            &base,
            Instant::now() + Duration::from_secs(5),
            "legacy-exclusive",
        )
        .expect("legacy exclusive at base");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "slot-peer",
        );
        assert!(
            contested.is_err(),
            "live legacy exclusive base must block slot children: {contested:?}"
        );
        drop(legacy);
        let after = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "after-legacy",
        );
        assert!(after.is_ok(), "slots acquire after legacy release: {after:?}");
        drop(after);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn acquire_slots_returns_fatal_when_parent_is_not_a_directory() {
        // Parent path is a file → create_dir for slot children fails as Fatal (not Busy).
        let blocker = std::env::temp_dir().join(format!(
            "tokenzero-permit-fatal-blocker-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_file(&blocker);
        let _ = fs::remove_dir_all(&blocker);
        fs::write(&blocker, b"not-a-directory").expect("write blocker file");
        let base = blocker.join("nested-permit");

        let err = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_millis(80),
            "test-fatal",
        )
        .expect_err("expected Fatal when permit parent is a file");
        let _ = fs::remove_file(&blocker);

        match err {
            AcquireError::Fatal(message) => {
                assert!(
                    message.contains("create codemode permit"),
                    "unexpected Fatal message: {message}"
                );
            }
            AcquireError::Busy(message) => {
                panic!("I/O failure must be Fatal, not Busy: {message}")
            }
        }
    }
}
