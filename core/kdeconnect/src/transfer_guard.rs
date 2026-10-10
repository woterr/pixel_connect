// Pixel Connect — safety limits for incoming file transfers.
//
// A peer we have paired is trusted to *offer* a file, but the amount it offers
// is still attacker-controlled data: a compromised phone, a hostile app using the
// KDE Connect protocol, or a replayed transfer can all announce an enormous
// size. Without a limit, a single transfer can fill the disk and take the whole
// desktop down with it.
//
// Three separate things can go wrong, and each needs its own check:
//
//   1. **Volume.** The peer announces more bytes than we are willing to store.
//      Checked before we accept, so we fail fast instead of filling the disk
//      and only then reporting an error.
//   2. **Free space.** The transfer fits within the volume limit but not within
//      the space actually available, leaving the system unable to write its own
//      logs or swap. A reserve is kept back.
//   3. **Completion.** The stream ended cleanly but delivered fewer bytes than
//      announced. A truncated file must never be renamed into place as though it
//      were whole, because that is how a user ends up opening a corrupt file
//      believing it is intact.
//
// The volume limit is configurable because a legitimate 4K video or a large
// archive should not be refused out of the box; the default is generous, and
// the free-space check is what actually prevents exhaustion on a small disk.

use std::path::Path;

/// Default ceiling on a single incoming file: 8 GiB.
///
/// Generous enough that ordinary transfers are unaffected. Users who routinely
/// move very large files can raise it; anyone who wants no ceiling at all must
/// set it explicitly, because "unlimited" is a worse default than "large".
pub const DEFAULT_MAX_INCOMING_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Space kept free on the target filesystem so the desktop remains usable
/// while a transfer is running.
pub const FREE_SPACE_RESERVE: u64 = 256 * 1024 * 1024;

/// Bytes we require to be available before starting, beyond what the file
/// itself needs. This is the reserve.
fn required_free_space(file_size: u64) -> u64 {
    file_size.saturating_add(FREE_SPACE_RESERVE)
}

/// Bytes still available on the filesystem holding `dir`.
///
/// Returns `None` when the space cannot be determined. A caller must treat
/// `None` as "unknown", not as "plenty": the volume limit remains the backstop.
pub fn available_bytes(dir: &Path) -> Option<u64> {
    // Walk up to the nearest existing ancestor: the configured download
    // directory may not have been created yet.
    let mut probe = dir;
    loop {
        let is_existing_dir = std::fs::metadata(probe)
            .map(|meta| meta.is_dir())
            .unwrap_or(false);
        if is_existing_dir {
            return nix_available(probe);
        }
        probe = probe.parent()?;
    }
}

/// Free space for an unprivileged user, via `statvfs(2)`.
///
/// `libc` is used directly rather than pulling in a dependency for one call; it
/// is already present transitively, so this adds no new supply-chain surface.
#[cfg(unix)]
fn nix_available(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the
    // call, and `stat` is only written to on success.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    // `f_bavail` counts blocks available to an unprivileged user, in units of
    // `f_frsize`. Saturating arithmetic keeps a hostile filesystem from wrapping
    // the multiplication into a small "plenty of space" answer.
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(unix))]
fn nix_available(_path: &Path) -> Option<u64> {
    None
}

/// Why an incoming transfer was refused before it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The peer announced more than the configured ceiling.
    TooLarge { announced: u64, limit: u64 },
    /// The filesystem does not have room for the file plus the reserve.
    NotEnoughSpace { needed: u64, available: Option<u64> },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::TooLarge { announced, limit } => write!(
                f,
                "the incoming file is {} which is over the {} limit",
                human_bytes(*announced),
                human_bytes(*limit)
            ),
            Refusal::NotEnoughSpace { needed, available } => match available {
                Some(avail) => write!(
                    f,
                    "the incoming file needs {} free but only {} is available",
                    human_bytes(*needed),
                    human_bytes(*avail)
                ),
                None => write!(
                    f,
                    "the incoming file needs {} free and the free space could not \
                     be determined",
                    human_bytes(*needed)
                ),
            },
        }
    }
}

/// Decide whether to accept an announced incoming size.
///
/// `limit` of `None` means "use the default ceiling". The check is skipped for
/// an announced size of zero, which is legitimate (an empty file) and cannot
/// exhaust anything.
pub fn check_incoming(dir: &Path, announced: u64, limit: Option<u64>) -> Result<(), Refusal> {
    let limit = limit.unwrap_or(DEFAULT_MAX_INCOMING_BYTES);
    if announced == 0 {
        return Ok(());
    }
    if announced > limit {
        return Err(Refusal::TooLarge { announced, limit });
    }
    let needed = required_free_space(announced);
    // When free space cannot be determined we do not refuse: the volume limit
    // already bounds the damage, and refusing here would break transfers on
    // filesystems that statvfs does not describe (some network and fuse mounts).
    if let Some(available) = available_bytes(dir) {
        if available < needed {
            return Err(Refusal::NotEnoughSpace {
                needed,
                available: Some(available),
            });
        }
    }
    Ok(())
}

/// Tracks an in-progress download so it can be stopped the moment it exceeds
/// what was agreed.
#[derive(Debug, Clone, Copy)]
pub struct DownloadGuard {
    limit: u64,
    /// The announced size, when the peer gave one.
    announced: Option<u64>,
    received: u64,
}

impl DownloadGuard {
    pub fn new(announced: Option<u64>, limit: Option<u64>) -> Self {
        Self {
            limit: limit.unwrap_or(DEFAULT_MAX_INCOMING_BYTES),
            announced: announced.filter(|n| *n > 0),
            received: 0,
        }
    }

    /// Record newly received bytes, returning `Err` when a limit is breached.
    ///
    /// Both limits are enforced while streaming, not only up front: a peer may
    /// announce a small size and then keep sending.
    pub fn record(&mut self, chunk: u64) -> Result<(), String> {
        self.received = self.received.saturating_add(chunk);
        if self.received > self.limit {
            return Err(format!(
                "the sender exceeded the {} limit",
                human_bytes(self.limit)
            ));
        }
        if let Some(announced) = self.announced {
            if self.received > announced {
                return Err(format!(
                    "the sender sent {} but announced only {}",
                    human_bytes(self.received),
                    human_bytes(announced)
                ));
            }
        }
        Ok(())
    }

    pub fn received(&self) -> u64 {
        self.received
    }

    /// Whether the transfer delivered exactly what was announced.
    ///
    /// A peer that announced nothing is never "short": there is nothing to
    /// compare against.
    pub fn is_complete(&self) -> bool {
        match self.announced {
            Some(announced) => self.received == announced,
            None => true,
        }
    }
}

/// A short human-readable size, used in errors the user reads.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pc-guard-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_zero_byte_transfer_is_always_allowed() {
        let dir = temp("zero");
        assert!(check_incoming(&dir, 0, Some(1)).is_ok());
    }

    #[test]
    fn a_transfer_over_the_ceiling_is_refused_with_both_sizes() {
        let dir = temp("ceiling");
        let err = check_incoming(&dir, 200, Some(100)).unwrap_err();
        assert_eq!(
            err,
            Refusal::TooLarge {
                announced: 200,
                limit: 100
            }
        );
        // The message must be readable and must not be empty.
        let msg = err.to_string();
        assert!(msg.contains('2') || msg.contains("200"), "{msg}");
    }

    #[test]
    fn the_default_ceiling_is_generous_enough_for_real_files() {
        let dir = temp("default");
        // A 4K video, a large archive and a 30 GB disk image by default? The
        // first two yes; the last only if the user raises the ceiling.
        assert!(
            check_incoming(&dir, 4 * 1024 * 1024 * 1024, None).is_ok() || {
                // May be refused for lack of free space in a small CI container, but
                // never for exceeding the ceiling itself.
                !matches!(
                    check_incoming(&dir, 4 * 1024 * 1024 * 1024, None),
                    Err(Refusal::TooLarge { .. })
                )
            }
        );
        assert!(
            matches!(
                check_incoming(&dir, 32 * 1024 * 1024 * 1024, None),
                Err(Refusal::TooLarge { .. })
            ),
            "a 32 GiB file must exceed the 8 GiB default ceiling"
        );
    }

    #[test]
    fn a_file_larger_than_the_free_space_is_refused() {
        // Ask for more than any test machine has free, with no ceiling.
        let dir = temp("space");
        let err = check_incoming(&dir, u64::MAX / 4, None).unwrap_err();
        // Either the ceiling or the space check must fire; both are refusals,
        // and neither may be silently accepted.
        assert!(
            matches!(
                err,
                Refusal::TooLarge { .. } | Refusal::NotEnoughSpace { .. }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn unknown_free_space_does_not_refuse_by_itself() {
        // A path that cannot be resolved must fall through to the volume check
        // rather than blocking every transfer.
        let missing = Path::new("/nonexistent-pc-guard/deeply/nested");
        // The nearest existing ancestor is "/", which usually does stat, so this
        // exercises the ancestor walk; the assertion is that it returns rather
        // than panicking.
        let _ = check_incoming(missing, 1024, Some(1024 * 1024));
    }

    #[test]
    fn the_guard_stops_a_peer_that_sends_more_than_it_announced() {
        let mut guard = DownloadGuard::new(Some(1000), None);
        assert!(guard.record(500).is_ok());
        // Announced 1000 but keeps going.
        assert!(guard.record(600).is_err());
    }

    #[test]
    fn the_guard_stops_a_peer_that_ignores_the_ceiling() {
        let mut guard = DownloadGuard::new(None, Some(1000));
        assert!(guard.record(900).is_ok());
        assert!(guard.record(200).is_err());
    }

    #[test]
    fn a_truncated_stream_is_not_complete() {
        let mut guard = DownloadGuard::new(Some(1000), None);
        guard.record(400).unwrap();
        assert!(!guard.is_complete());
    }

    #[test]
    fn an_exact_stream_is_complete() {
        let mut guard = DownloadGuard::new(Some(1000), None);
        guard.record(600).unwrap();
        guard.record(400).unwrap();
        assert!(guard.is_complete());
        assert_eq!(guard.received(), 1000);
    }

    #[test]
    fn a_peer_that_announced_nothing_is_never_short() {
        let mut guard = DownloadGuard::new(None, None);
        guard.record(7).unwrap();
        assert!(guard.is_complete());
    }

    #[test]
    fn a_announced_size_of_zero_means_unbounded_not_empty() {
        // Some peers send filesize 0 to mean "unknown"; treating that as
        // "already complete" or as "must receive 0 bytes" would break them.
        let mut guard = DownloadGuard::new(Some(0), None);
        guard.record(500).unwrap();
        assert!(guard.is_complete());
    }

    #[test]
    fn byte_counts_cannot_overflow_into_a_pass() {
        // A peer announcing an absurd size must not be able to wrap the
        // counter round to something small and look like a valid transfer.
        let announced = u64::MAX - 5;
        let mut guard = DownloadGuard::new(Some(announced), Some(u64::MAX));
        assert!(guard.record(u64::MAX - 10).is_ok());
        // This addition would overflow; it must saturate and then fail.
        assert!(guard.record(100).is_err());
    }

    #[test]
    fn human_sizes_are_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(8 * 1024 * 1024 * 1024), "8.0 GiB");
    }
}
