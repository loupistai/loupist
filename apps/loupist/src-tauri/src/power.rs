//! Keep-awake while World work remains.
//!
//! The host holds one macOS idle-sleep assertion while the user enabled the
//! application-wide setting, World jobs are pending and the Mac is not running
//! on battery. It prevents idle *system* sleep only: the display may still
//! sleep and closing the lid still sleeps the Mac. Other platforms report the
//! feature as unsupported and never hold anything.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;

use crate::store::{SqliteStore, StoreError};

/// How often the scheduler re-evaluates while holding (or wanting) the
/// assertion, so unplugging releases it and plugging back in re-acquires it.
const POWER_POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Shown by `pmset -g assertions` and Activity Monitor's energy details.
const ASSERTION_REASON: &str = "Loupist is processing saved Sources into the World";

/// World work the scheduler observed after its latest step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Demand {
    pub pending: bool,
    pub remaining: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeepAwakeStatus {
    pub supported: bool,
    pub enabled: bool,
    pub active: bool,
    pub on_battery: Option<bool>,
    pub remaining: u64,
}

/// Platform power management. Holds release when the returned object drops.
pub(crate) trait PowerBackend: Send + Sync {
    fn supported(&self) -> bool;
    /// `None` when the providing power source is unknown.
    fn on_ac_power(&self) -> Option<bool>;
    /// `None` when the platform refused the hold; a later evaluation retries.
    fn hold(&self, reason: &str) -> Option<Box<dyn Any + Send>>;
}

#[derive(Clone)]
pub(crate) struct KeepAwake {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<SqliteStore>,
    backend: Arc<dyn PowerBackend>,
    /// Orders setting writes so the cache matches the last saved value; held
    /// apart from `state` so scheduling steps never wait for SQLite.
    writes: Mutex<()>,
    state: Mutex<State>,
}

struct State {
    enabled: bool,
    demand: Demand,
    hold: Option<Box<dyn Any + Send>>,
    reported_hold_failure: bool,
}

impl KeepAwake {
    // Test builds use a fake backend (see `graph_service`).
    #[cfg_attr(test, allow(dead_code))]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self::build(store, platform_backend())
    }

    #[cfg(test)]
    pub fn with_backend(store: Arc<SqliteStore>, backend: Arc<dyn PowerBackend>) -> Self {
        Self::build(store, backend)
    }

    fn build(store: Arc<SqliteStore>, backend: Arc<dyn PowerBackend>) -> Self {
        // Read once; `set_enabled` keeps the cache current, so scheduling
        // steps never touch SQLite.
        let enabled = store.keep_awake_enabled().unwrap_or_else(|error| {
            eprintln!("Could not read the keep-awake setting: {error}");
            false
        });
        Self {
            inner: Arc::new(Inner {
                store,
                backend,
                writes: Mutex::new(()),
                state: Mutex::new(State {
                    enabled,
                    demand: Demand::default(),
                    hold: None,
                    reported_hold_failure: false,
                }),
            }),
        }
    }

    /// Re-evaluates after a scheduling step: acquires or releases the hold.
    pub fn update(&self, demand: Demand) {
        let mut state = self.state();
        state.demand = demand;
        self.reconcile(&mut state, false);
    }

    /// Persists the application-wide choice, then re-evaluates the last demand.
    pub fn set_enabled(&self, enabled: bool) -> Result<KeepAwakeStatus, StoreError> {
        let _write = lock(&self.inner.writes);
        self.inner.store.set_keep_awake(enabled)?;
        let mut state = self.state();
        state.enabled = enabled;
        let on_ac = self.reconcile(&mut state, true);
        Ok(self.describe(&state, on_ac))
    }

    pub fn status(&self) -> KeepAwakeStatus {
        let mut state = self.state();
        let on_ac = self.reconcile(&mut state, true);
        self.describe(&state, on_ac)
    }

    /// The saved choice, without reading the power source.
    pub fn enabled(&self) -> bool {
        self.state().enabled
    }

    /// `Some` while a hold is wanted, so the scheduler notices AC/battery changes.
    pub fn poll_interval(&self) -> Option<Duration> {
        let state = self.state();
        (state.enabled && state.demand.pending && self.inner.backend.supported())
            .then_some(POWER_POLL_INTERVAL)
    }

    /// Drops any hold at shutdown. Only a later `update` reporting pending
    /// work may acquire again; `status` and `set_enabled` do not.
    pub fn release(&self) {
        let mut state = self.state();
        state.demand = Demand::default();
        state.hold = None;
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }

    /// Holds exactly while enabled, supported, pending and not on battery.
    /// Reads the power source only when a hold is wanted, or for a status
    /// report; returns it (`None` when not read or unknown).
    fn reconcile(&self, state: &mut State, report_power: bool) -> Option<bool> {
        let backend = &self.inner.backend;
        let wanted = state.enabled && state.demand.pending && backend.supported();
        let on_ac = if wanted || (report_power && backend.supported()) {
            backend.on_ac_power()
        } else {
            None
        };
        // An unknown power source counts as AC; a laptop on battery never holds.
        if !wanted || on_ac == Some(false) {
            state.hold = None;
            return on_ac;
        }
        if state.hold.is_none() {
            state.hold = backend.hold(ASSERTION_REASON);
            if state.hold.is_none() && !state.reported_hold_failure {
                eprintln!("Could not keep this Mac awake for World processing");
            }
            state.reported_hold_failure = state.hold.is_none();
        }
        on_ac
    }

    fn describe(&self, state: &State, on_ac: Option<bool>) -> KeepAwakeStatus {
        KeepAwakeStatus {
            supported: self.inner.backend.supported(),
            enabled: state.enabled,
            active: state.hold.is_some(),
            on_battery: on_ac.map(|ac| !ac),
            remaining: state.demand.remaining,
        }
    }
}

// Every step leaves the state consistent; a panic elsewhere must not stop
// releasing the assertion.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
fn platform_backend() -> Arc<dyn PowerBackend> {
    Arc::new(macos::MacPower)
}

#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
fn platform_backend() -> Arc<dyn PowerBackend> {
    Arc::new(Unsupported)
}

#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
struct Unsupported;

#[cfg(not(target_os = "macos"))]
impl PowerBackend for Unsupported {
    fn supported(&self) -> bool {
        false
    }

    fn on_ac_power(&self) -> Option<bool> {
        None
    }

    fn hold(&self, _reason: &str) -> Option<Box<dyn Any + Send>> {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! IOKit power assertions and power-source reads through raw FFI.
    use std::any::Any;
    use std::ffi::{c_char, c_void, CStr, CString};

    use super::PowerBackend;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type IOPMAssertionID = u32;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    const K_IO_RETURN_SUCCESS: i32 = 0;
    /// Prevents idle system sleep only. The display may sleep; the lid still sleeps.
    const ASSERTION_TYPE: &str = "PreventUserIdleSystemSleep";

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            value: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(
            value: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: CFTypeRef);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            level: u32,
            name: CFStringRef,
            id: *mut IOPMAssertionID,
        ) -> i32;
        fn IOPMAssertionRelease(id: IOPMAssertionID) -> i32;
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    }

    pub(super) struct MacPower;

    /// An owned assertion; dropping it releases the hold.
    struct Assertion(IOPMAssertionID);

    impl Drop for Assertion {
        fn drop(&mut self) {
            // SAFETY: the id came from a successful create and is released once.
            unsafe {
                IOPMAssertionRelease(self.0);
            }
        }
    }

    /// An owned CFString (Create rule); dropping it releases the reference.
    struct OwnedString(CFStringRef);

    impl OwnedString {
        fn new(value: &str) -> Option<Self> {
            let value = CString::new(value).ok()?;
            // SAFETY: `value` is a NUL-terminated UTF-8 string that outlives the call.
            let string = unsafe {
                CFStringCreateWithCString(
                    std::ptr::null(),
                    value.as_ptr(),
                    K_CF_STRING_ENCODING_UTF8,
                )
            };
            (!string.is_null()).then_some(Self(string))
        }
    }

    impl Drop for OwnedString {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns exactly one reference to a non-null CFString.
            unsafe { CFRelease(self.0) }
        }
    }

    /// Copies a borrowed CFString (Get rule) without taking ownership.
    fn read_string(value: CFStringRef) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut buffer = [0 as c_char; 64];
        // SAFETY: `value` is a live CFString and the buffer length is exact.
        let copied = unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        if copied == 0 {
            return None;
        }
        // SAFETY: a successful copy wrote a NUL-terminated string into `buffer`.
        let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        text.to_str().ok().map(str::to_owned)
    }

    impl PowerBackend for MacPower {
        fn supported(&self) -> bool {
            true
        }

        fn on_ac_power(&self) -> Option<bool> {
            // SAFETY: the snapshot follows the Copy rule and is released below;
            // the providing type is borrowed from it and read before release.
            let kind = unsafe {
                let snapshot = IOPSCopyPowerSourcesInfo();
                if snapshot.is_null() {
                    return None;
                }
                let kind = read_string(IOPSGetProvidingPowerSourceType(snapshot));
                CFRelease(snapshot);
                kind
            };
            match kind.as_deref() {
                Some("AC Power") => Some(true),
                // A UPS on its own battery means wall power is gone.
                Some("Battery Power" | "UPS Power") => Some(false),
                _ => None,
            }
        }

        fn hold(&self, reason: &str) -> Option<Box<dyn Any + Send>> {
            let kind = OwnedString::new(ASSERTION_TYPE)?;
            let name = OwnedString::new(reason)?;
            let mut id: IOPMAssertionID = 0;
            // SAFETY: both strings are live CFStrings; IOKit copies them and
            // writes the new assertion id on success.
            let result = unsafe {
                IOPMAssertionCreateWithName(kind.0, K_IOPM_ASSERTION_LEVEL_ON, name.0, &mut id)
            };
            (result == K_IO_RETURN_SUCCESS).then(|| Box::new(Assertion(id)) as Box<dyn Any + Send>)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::ASSERTION_REASON;
        use super::*;

        fn pmset(args: &[&str]) -> String {
            let output = std::process::Command::new("/usr/bin/pmset")
                .args(args)
                .output()
                .expect("pmset runs on macOS");
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        fn listed(pid: u32) -> Option<String> {
            let pid = format!("pid {pid}(");
            pmset(&["-g", "assertions"])
                .lines()
                .find(|line| line.contains(&pid) && line.contains(ASSERTION_REASON))
                .map(|line| line.trim().to_owned())
        }

        #[test]
        fn power_source_matches_pmset() {
            let expected = match pmset(&["-g", "ps"]).lines().next() {
                Some(line) if line.contains("'AC Power'") => Some(true),
                Some(line) if line.contains("'Battery Power'") || line.contains("'UPS Power'") => {
                    Some(false)
                }
                _ => None,
            };
            assert_eq!(MacPower.on_ac_power(), expected);
        }

        // Holds a real assertion for a moment and proves IOKit lists, then drops, it.
        #[test]
        fn real_assertion_is_listed_while_held_and_gone_after_drop() {
            let pid = std::process::id();
            let hold = MacPower
                .hold(ASSERTION_REASON)
                .expect("IOKit creates the idle-sleep assertion");
            let line = listed(pid).expect("pmset lists the held assertion");
            assert!(line.contains(ASSERTION_TYPE), "{line}");
            eprintln!("pmset while held: {line}");
            drop(hold);
            let released = (0..20).any(|_| {
                if listed(pid).is_none() {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
                false
            });
            assert!(released, "pmset still lists the released assertion");
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::any::Any;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::PowerBackend;

    /// A counting backend: tests flip power and support and watch holds.
    pub(crate) struct FakePower {
        pub supported: AtomicBool,
        pub on_ac: Mutex<Option<bool>>,
        pub holds: Arc<AtomicUsize>,
        pub releases: Arc<AtomicUsize>,
        pub reasons: Mutex<Vec<String>>,
    }

    struct FakeHold(Arc<AtomicUsize>);

    impl Drop for FakeHold {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl FakePower {
        pub(crate) fn new(supported: bool) -> Arc<Self> {
            Arc::new(Self {
                supported: AtomicBool::new(supported),
                on_ac: Mutex::new(Some(true)),
                holds: Arc::default(),
                releases: Arc::default(),
                reasons: Mutex::default(),
            })
        }

        pub(crate) fn set_on_ac(&self, on_ac: Option<bool>) {
            *self.on_ac.lock().unwrap() = on_ac;
        }

        pub(crate) fn holds(&self) -> usize {
            self.holds.load(Ordering::SeqCst)
        }

        pub(crate) fn releases(&self) -> usize {
            self.releases.load(Ordering::SeqCst)
        }

        /// Holds currently alive.
        pub(crate) fn held(&self) -> usize {
            self.holds() - self.releases()
        }
    }

    impl PowerBackend for FakePower {
        fn supported(&self) -> bool {
            self.supported.load(Ordering::SeqCst)
        }

        fn on_ac_power(&self) -> Option<bool> {
            *self.on_ac.lock().unwrap()
        }

        fn hold(&self, reason: &str) -> Option<Box<dyn Any + Send>> {
            self.holds.fetch_add(1, Ordering::SeqCst);
            self.reasons.lock().unwrap().push(reason.to_owned());
            Some(Box::new(FakeHold(self.releases.clone())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakePower;
    use super::*;

    fn store() -> (tempfile::TempDir, Arc<SqliteStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(dir.path().join("power.sqlite3")).unwrap());
        (dir, store)
    }

    fn enabled(power: &Arc<FakePower>) -> (tempfile::TempDir, KeepAwake) {
        let (dir, store) = store();
        store.set_keep_awake(true).unwrap();
        (dir, KeepAwake::with_backend(store, power.clone()))
    }

    const PENDING: Demand = Demand {
        pending: true,
        remaining: 3,
    };
    const DRAINED: Demand = Demand {
        pending: false,
        remaining: 0,
    };

    #[test]
    fn holds_once_while_work_is_pending_and_releases_when_drained() {
        let power = FakePower::new(true);
        let (_dir, keep_awake) = enabled(&power);
        keep_awake.update(PENDING);
        keep_awake.update(Demand {
            pending: true,
            remaining: 2,
        });
        assert_eq!((power.holds(), power.held()), (1, 1));
        assert_eq!(power.reasons.lock().unwrap().as_slice(), [ASSERTION_REASON]);
        keep_awake.update(DRAINED);
        assert_eq!((power.holds(), power.held()), (1, 0));
        keep_awake.update(DRAINED);
        assert_eq!(power.releases(), 1);
    }

    #[test]
    fn disabled_or_unsupported_never_holds() {
        let power = FakePower::new(true);
        let (_dir, store) = store();
        let keep_awake = KeepAwake::with_backend(store, power.clone());
        keep_awake.update(PENDING);
        assert_eq!(keep_awake.poll_interval(), None);
        assert!(!keep_awake.status().active);
        assert_eq!(power.holds(), 0);

        let unsupported = FakePower::new(false);
        let (_dir, keep_awake) = enabled(&unsupported);
        keep_awake.update(PENDING);
        let status = keep_awake.status();
        assert!(!status.supported && status.enabled && !status.active);
        assert_eq!(status.on_battery, None);
        assert_eq!(keep_awake.poll_interval(), None);
        assert_eq!(unsupported.holds(), 0);
    }

    #[test]
    fn battery_releases_and_ac_or_unknown_power_reacquires() {
        let power = FakePower::new(true);
        let (_dir, keep_awake) = enabled(&power);
        keep_awake.update(PENDING);
        assert_eq!(power.held(), 1);
        power.set_on_ac(Some(false));
        keep_awake.update(PENDING);
        assert_eq!(power.held(), 0);
        let status = keep_awake.status();
        assert_eq!((status.active, status.on_battery), (false, Some(true)));
        // Still pending on battery: keep polling to notice the charger.
        assert_eq!(keep_awake.poll_interval(), Some(POWER_POLL_INTERVAL));
        power.set_on_ac(Some(true));
        keep_awake.update(PENDING);
        assert_eq!((power.holds(), power.held()), (2, 1));
        power.set_on_ac(None);
        keep_awake.update(PENDING);
        assert_eq!((power.holds(), power.held()), (2, 1));
        assert_eq!(keep_awake.status().on_battery, None);
    }

    #[test]
    fn set_enabled_persists_and_applies_immediately() {
        let power = FakePower::new(true);
        let (_dir, store) = store();
        let keep_awake = KeepAwake::with_backend(store.clone(), power.clone());
        keep_awake.update(PENDING);
        assert_eq!(power.holds(), 0);
        let status = keep_awake.set_enabled(true).unwrap();
        assert!(status.enabled && status.active);
        assert_eq!(status.remaining, 3);
        assert!(store.keep_awake_enabled().unwrap());
        assert_eq!(power.held(), 1);
        let status = keep_awake.set_enabled(false).unwrap();
        assert!(!status.enabled && !status.active);
        assert_eq!(power.held(), 0);
        assert!(!store.keep_awake_enabled().unwrap());
        // A restart reads the saved choice once.
        store.set_keep_awake(true).unwrap();
        let restarted = KeepAwake::with_backend(store, power.clone());
        assert!(restarted.status().enabled);
    }

    #[test]
    fn release_drops_the_hold_until_the_scheduler_reports_work_again() {
        let power = FakePower::new(true);
        let (_dir, keep_awake) = enabled(&power);
        keep_awake.update(PENDING);
        keep_awake.release();
        assert_eq!(power.held(), 0);
        // Reads and toggles after shutdown must not re-acquire on stale demand.
        assert!(!keep_awake.status().active);
        keep_awake.set_enabled(true).unwrap();
        assert_eq!(power.held(), 0);
        assert_eq!(keep_awake.poll_interval(), None);
        keep_awake.update(PENDING);
        assert_eq!((power.holds(), power.held()), (2, 1));
    }

    #[test]
    fn status_reports_every_field() {
        let power = FakePower::new(true);
        let (_dir, keep_awake) = enabled(&power);
        keep_awake.update(Demand {
            pending: true,
            remaining: 7,
        });
        let status = keep_awake.status();
        assert!(status.supported && status.enabled && status.active);
        assert_eq!((status.on_battery, status.remaining), (Some(false), 7));
        assert_eq!(keep_awake.poll_interval(), Some(POWER_POLL_INTERVAL));
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            serde_json::json!({"supported": true, "enabled": true, "active": true, "onBattery": false, "remaining": 7})
        );
    }
}
