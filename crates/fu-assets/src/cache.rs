use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};

use fu_vfs::{CaseSensitivity, MountInfo, Vfs, VfsPath, VfsWriteTarget};

use crate::asset::Asset;
use crate::error::{AssetError, AssetResult, PinInvalidation};

#[cfg(test)]
pub(crate) mod test_hooks {
    //! Deterministic rendezvous points for concurrency regression tests.
    //!
    //! Compiled only under `cfg(test)`; no code is emitted into release builds and
    //! no public API is affected. A site only parks a thread when it has been
    //! armed for the exact `AssetCache` instance performing the operation, so
    //! tests running in parallel cannot interfere with one another.

    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Site {
        /// `load` step 2: `in_flight` write guard held, before the `router`/`slots` re-check.
        LoadStep2HoldingInFlight,
        /// `load` step 4: the `slots` read guard has been released; before install.
        LoadStep4BeforeInstall,
        /// `evict_path`: entry point, before the `in_flight` write guard.
        EvictPathEntry,
        /// `pin`: after `load` returns, before pin verification locks are taken.
        PinAfterLoadBeforeVerify,
    }

    struct Slot {
        owner: Option<usize>,
        /// Accept new arrivals. Cleared by `disarm` so the *next* thread through
        /// the same site is not trapped, while an already parked thread stays put.
        armed: bool,
        reached: bool,
        released: bool,
    }

    struct SiteState {
        slot: Mutex<Slot>,
        cv: Condvar,
    }

    impl SiteState {
        const fn new() -> Self {
            Self {
                slot: Mutex::new(Slot {
                    owner: None,
                    armed: false,
                    reached: false,
                    released: false,
                }),
                cv: Condvar::new(),
            }
        }
    }

    static SITES: [SiteState; 4] = [
        SiteState::new(),
        SiteState::new(),
        SiteState::new(),
        SiteState::new(),
    ];

    fn idx(site: Site) -> usize {
        match site {
            Site::LoadStep2HoldingInFlight => 0,
            Site::LoadStep4BeforeInstall => 1,
            Site::EvictPathEntry => 2,
            Site::PinAfterLoadBeforeVerify => 3,
        }
    }

    pub(crate) fn arm(site: Site, owner: usize) {
        let st = &SITES[idx(site)];
        let mut g = st.slot.lock().unwrap_or_else(|p| p.into_inner());
        g.owner = Some(owner);
        g.armed = true;
        g.reached = false;
        g.released = false;
    }

    /// Stop trapping new arrivals at this site. A thread already parked by
    /// `check` stays parked until `release`.
    pub(crate) fn disarm(site: Site) {
        let st = &SITES[idx(site)];
        let mut g = st.slot.lock().unwrap_or_else(|p| p.into_inner());
        g.armed = false;
    }

    pub(crate) fn release(site: Site) {
        let st = &SITES[idx(site)];
        let mut g = st.slot.lock().unwrap_or_else(|p| p.into_inner());
        g.armed = false;
        g.released = true;
        st.cv.notify_all();
    }

    pub(crate) fn wait_reached(site: Site, timeout: Duration) -> bool {
        let st = &SITES[idx(site)];
        let start = Instant::now();
        let mut g = st.slot.lock().unwrap_or_else(|p| p.into_inner());
        while !g.reached {
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                return false;
            }
            let (next, _) = st
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
        }
        true
    }

    /// Called from cache internals. Parks the calling thread only when this site
    /// is armed for the `AssetCache` instance performing the operation.
    pub(crate) fn check(site: Site, owner: usize) {
        let st = &SITES[idx(site)];
        let mut g = st.slot.lock().unwrap_or_else(|p| p.into_inner());
        if !g.armed || g.released || g.owner != Some(owner) {
            return;
        }
        g.reached = true;
        st.cv.notify_all();
        let start = Instant::now();
        while !g.released {
            let left = Duration::from_secs(10).saturating_sub(start.elapsed());
            if left.is_zero() {
                break;
            }
            let (next, _) = st
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
        }
    }
}

/// Canonical cache identity of a decoded asset instance.
///
/// Uniquely disambiguates assets across:
/// 1. Target type (`type_id`).
/// 2. Resolving VFS mount (`mount_id`).
/// 3. Normalized relative path with backend casing (`canonical_relative_path`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CanonicalKey {
    pub type_id: TypeId,
    pub mount_id: String,
    pub canonical_relative_path: String,
}

/// Request cache key identifying an asset lookup by caller virtual path and type.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RequestKey {
    pub type_id: TypeId,
    pub path: VfsPath,
}

/// Maximum number of snapshot retries before a case-aware eviction gives up.
const MAX_SNAPSHOT_ATTEMPTS: usize = 8;

/// Upper bound on retained eviction intents. If exceeded, the oldest intents are
/// dropped and `journal_floor` conservatively rejects any still-older in-flight
/// load's installation.
const MAX_EVICTION_INTENTS: usize = 4096;

/// Snapshot of the topology facts that determined a cache entry's provenance.
#[derive(Clone, Debug)]
struct MountStamp {
    id: String,
    /// Unique instance of this mount. A remount under the same id receives a new
    /// value; enable/disable keeps it stable.
    instance_id: u64,
    mount_point: VfsPath,
    priority: i32,
    case_sensitivity: CaseSensitivity,
}

impl MountStamp {
    fn from_info(info: &MountInfo, instance_id: u64) -> Self {
        Self {
            id: info.id.clone(),
            instance_id,
            mount_point: info.mount_point.clone(),
            priority: info.priority,
            case_sensitivity: info.case_sensitivity,
        }
    }
}

/// A cached decoded value plus the mount instance that produced it.
struct SlotEntry {
    weak: Weak<dyn Any + Send + Sync>,
    mount: MountStamp,
}

impl SlotEntry {
    fn new(handle: &Arc<dyn Any + Send + Sync>, mount: &MountStamp) -> Self {
        Self {
            weak: Arc::downgrade(handle),
            mount: mount.clone(),
        }
    }

    fn upgrade(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        self.weak.upgrade()
    }
}

/// A pinned handle plus the mount instance that produced it.
struct PinnedHandle {
    handle: Arc<dyn Any + Send + Sync>,
    mount: MountStamp,
}

/// A retained handle plus the mount instance that produced it.
struct RetainedHandle {
    _handle: Arc<dyn Any + Send + Sync>,
    mount: MountStamp,
}

#[derive(Clone, Debug)]
struct RouteEntry {
    canonical: CanonicalKey,
    generation: u64,
    mount: MountStamp,
}

#[derive(Clone, Debug)]
struct PinnedRoute {
    canonical: CanonicalKey,
    mount: MountStamp,
}

struct InFlightEntry {
    barrier: Arc<InFlightBarrier>,
    start_seq: u64,
}

#[derive(Clone, Debug)]
struct EvictionIntent {
    seq: u64,
    topology_generation: u64,
    target: VfsPath,
    target_mount: Option<MountStamp>,
}

struct InstallContext<'a> {
    mount: &'a MountStamp,
    gen_used_for_bytes: u64,
    current_vfs_gen: u64,
}

/// Splits a canonical relative path into components. Empty paths have no components.
fn component_prefix(a: &str, b: &str, case: CaseSensitivity) -> bool {
    let mut ai = a.split('/').filter(|s| !s.is_empty());
    let mut bi = b.split('/').filter(|s| !s.is_empty());
    loop {
        match (ai.next(), bi.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(x), Some(y)) => {
                let equal = match case {
                    CaseSensitivity::Sensitive => x == y,
                    CaseSensitivity::Insensitive => x.eq_ignore_ascii_case(y),
                };
                if !equal {
                    return false;
                }
            }
        }
    }
}

fn identity_of(key: &CanonicalKey) -> (String, String) {
    (key.mount_id.clone(), key.canonical_relative_path.clone())
}

fn mount_stamp_for(vfs: &Vfs, mount_id: &str) -> Option<MountStamp> {
    let info = vfs.get_mount_info(mount_id)?;
    let instance_id = vfs.mount_instance_id(mount_id)?;
    Some(MountStamp::from_info(&info, instance_id))
}

fn mount_stamp_from_target(target: &VfsWriteTarget) -> Option<MountStamp> {
    let mount = target.mount.as_ref()?;
    let instance_id = target.mount_instance?;
    Some(MountStamp::from_info(mount, instance_id))
}

fn pin_invalidated(path: &VfsPath, reason: PinInvalidation) -> AssetError {
    AssetError::PinInvalidated {
        path: path.clone(),
        reason,
    }
}

/// Authority ordering identical to `Vfs::sorted_candidate_mounts`:
/// longest mount point, then highest priority, then lowest mount id.
fn outranks(t: &MountStamp, m: &MountStamp) -> bool {
    let tl = t.mount_point.as_str().len();
    let ml = m.mount_point.as_str().len();
    if tl != ml {
        return tl > ml;
    }
    if t.priority != m.priority {
        return t.priority > m.priority;
    }
    t.id < m.id
}

/// True when `target` and the cached canonical identity refer to the same file
/// or subtree under `target_mount`.
fn same_mount_identity_affected(
    canonical: &CanonicalKey,
    target: &VfsPath,
    target_mount: &MountStamp,
) -> bool {
    if canonical.mount_id != target_mount.id {
        return false;
    }
    match target.strip_prefix(&target_mount.mount_point) {
        Some(relative_target) => component_prefix(
            relative_target,
            &canonical.canonical_relative_path,
            target_mount.case_sensitivity,
        ),
        None => false,
    }
}

/// Reconstruct the virtual path represented by a canonical identity using the
/// mount stamp recorded when the entry was created. This works even after the
/// mount has been unmounted.
fn canonical_virtual_path(key: &CanonicalKey, mount: &MountStamp) -> Option<VfsPath> {
    mount.mount_point.join(&key.canonical_relative_path).ok()
}

fn canonical_is_under_target(key: &CanonicalKey, mount: &MountStamp, target: &VfsPath) -> bool {
    canonical_virtual_path(key, mount).is_some_and(|path| path.starts_with(target))
}

/// The core invalidation predicate shared by cached entries and in-flight intents.
fn entry_affected(
    request: &VfsPath,
    canonical: &CanonicalKey,
    mount: &MountStamp,
    target: &VfsPath,
    target_mount: Option<&MountStamp>,
) -> bool {
    if request.starts_with(target) {
        return true;
    }

    let Some(target_mount) = target_mount else {
        return false;
    };

    if mount.instance_id == target_mount.instance_id {
        return same_mount_identity_affected(canonical, target, target_mount);
    }

    // A write through a higher-authority mount can shadow a lower fallback route
    // for the same virtual path even when the request casing differs.
    if outranks(target_mount, mount) && request.starts_with(&target_mount.mount_point) {
        let Some(relative_target) = target.strip_prefix(&target_mount.mount_point) else {
            return false;
        };
        let relative_request = request
            .strip_prefix(&target_mount.mount_point)
            .unwrap_or_default();
        return component_prefix(
            relative_target,
            relative_request,
            target_mount.case_sensitivity,
        );
    }

    false
}

/// Whether the cached bytes themselves were changed by the write, as opposed to
/// only the route being shadowed by a higher-authority mount.
fn identity_affected_by_entry(
    request: &VfsPath,
    canonical: &CanonicalKey,
    mount: &MountStamp,
    target: &VfsPath,
    target_mount: Option<&MountStamp>,
) -> bool {
    if request.starts_with(target) {
        return true;
    }
    target_mount.is_some_and(|target_mount| {
        mount.instance_id == target_mount.instance_id
            && same_mount_identity_affected(canonical, target, target_mount)
    })
}

fn intent_affects(
    intent: &EvictionIntent,
    request: &VfsPath,
    canonical: &CanonicalKey,
    mount: &MountStamp,
    generation: u64,
) -> bool {
    if intent.topology_generation != generation {
        return false;
    }
    entry_affected(
        request,
        canonical,
        mount,
        &intent.target,
        intent.target_mount.as_ref(),
    )
}

type InFlightResult = Result<Arc<dyn Any + Send + Sync>, Arc<AssetError>>;

struct InFlightBarrier {
    mutex: Mutex<Option<InFlightResult>>,
    condvar: Condvar,
}

impl InFlightBarrier {
    fn new() -> Self {
        Self {
            mutex: Mutex::new(None),
            condvar: Condvar::new(),
        }
    }

    fn wait_result(&self) -> InFlightResult {
        let mut state = self.mutex.lock().unwrap_or_else(|p| p.into_inner());
        while state.is_none() {
            state = self.condvar.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        state.as_ref().unwrap().clone()
    }

    fn publish(&self, result: InFlightResult) {
        let mut state = self.mutex.lock().unwrap_or_else(|p| p.into_inner());
        *state = Some(result);
        self.condvar.notify_all();
    }
}

struct LoadingGuard<'a> {
    cache: &'a AssetCache,
    request_key: RequestKey,
    canonical_key: Option<CanonicalKey>,
    request_barrier: Arc<InFlightBarrier>,
    canonical_barrier: Option<Arc<InFlightBarrier>>,
    success: bool,
}

impl<'a> Drop for LoadingGuard<'a> {
    fn drop(&mut self) {
        if !self.success {
            // Panic or abnormal abort: publish LoadAborted so waiters never deadlock
            let err = Arc::new(AssetError::LoadAborted(self.request_key.path.clone()));

            self.request_barrier.publish(Err(err.clone()));
            self.cache
                .remove_in_flight_if_matching(&self.request_key, &self.request_barrier);

            if let (Some(can_key), Some(can_bar)) = (&self.canonical_key, &self.canonical_barrier) {
                can_bar.publish(Err(err));
                self.cache
                    .remove_canonical_in_flight_if_matching(can_key, can_bar);
            }
        }
    }
}

/// Bounded least-recently-used retention ring for decoded assets.
///
/// Keeps strong references to recently accessed unpinned assets across scenes
/// and song transitions, avoiding deallocation and re-decoding spikes.
struct RetentionRing {
    queue: VecDeque<CanonicalKey>,
    set: BTreeSet<CanonicalKey>,
    retained_handles: BTreeMap<CanonicalKey, RetainedHandle>,
    /// Canonical keys whose strong handle was dropped by eviction and whose
    /// `slots` entry must be pruned on the next `slots` write acquisition.
    dead_keys: Vec<CanonicalKey>,
    capacity: usize,
}

impl RetentionRing {
    fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            set: BTreeSet::new(),
            retained_handles: BTreeMap::new(),
            dead_keys: Vec::new(),
            capacity,
        }
    }

    fn touch(&mut self, key: CanonicalKey, handle: Arc<dyn Any + Send + Sync>, mount: MountStamp) {
        if self.capacity == 0 {
            return;
        }

        if self.set.contains(&key) {
            // Move to MRU position (back of queue)
            self.queue.retain(|k| k != &key);
        } else {
            // Evict oldest entries until strictly below capacity
            while self.queue.len() >= self.capacity {
                if let Some(oldest) = self.queue.pop_front() {
                    self.set.remove(&oldest);
                    self.retained_handles.remove(&oldest);
                    self.dead_keys.push(oldest);
                }
            }
        }

        self.set.insert(key.clone());
        self.queue.push_back(key.clone());
        self.retained_handles.insert(
            key,
            RetainedHandle {
                _handle: handle,
                mount,
            },
        );
    }

    fn touch_existing(&mut self, key: &CanonicalKey) -> bool {
        if self.set.contains(key) {
            self.queue.retain(|k| k != key);
            self.queue.push_back(key.clone());
            true
        } else {
            false
        }
    }

    fn remove(&mut self, key: &CanonicalKey) {
        if self.set.remove(key) {
            self.queue.retain(|k| k != key);
            self.retained_handles.remove(key);
        }
    }

    fn clear(&mut self) {
        let drained: Vec<CanonicalKey> = self.queue.drain(..).collect();
        self.dead_keys.extend(drained);
        self.set.clear();
        self.retained_handles.clear();
    }

    fn set_capacity(&mut self, new_capacity: usize) {
        self.capacity = new_capacity;
        if self.capacity == 0 {
            self.clear();
        } else {
            while self.queue.len() > self.capacity {
                if let Some(oldest) = self.queue.pop_front() {
                    self.set.remove(&oldest);
                    self.retained_handles.remove(&oldest);
                    self.dead_keys.push(oldest);
                }
            }
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.retained_handles.len()
    }
}

/// Central asset cache for FNF Universal.
///
/// Provides:
/// - Exact canonical cache identity: `(TypeId, mount_id, canonical_relative_path)`.
/// - Request path routing with VFS mount topology invalidation.
/// - Concurrent in-flight loading deduplication with shared results and shared errors.
/// - Double-checked canonical decode coordination preventing duplicate decodes across casing variants.
/// - Weak-reference slot ownership ensuring zero memory leaks upon LRU eviction.
/// - Bounded LRU retention ring for song transitions and scene reuse.
/// - Explicit strong pinning for permanent global assets (UI fonts, HUD skins, menu audio),
///   preserved independently of route invalidation.
/// - Thread-safe, non-panicking poison recovery across all synchronization primitives.
pub struct AssetCache {
    slots: RwLock<BTreeMap<CanonicalKey, SlotEntry>>,
    in_flight: RwLock<BTreeMap<RequestKey, InFlightEntry>>,
    canonical_in_flight: RwLock<BTreeMap<CanonicalKey, Arc<InFlightBarrier>>>,
    router: RwLock<BTreeMap<RequestKey, RouteEntry>>,
    pinned: RwLock<BTreeMap<CanonicalKey, PinnedHandle>>,
    pinned_requests: RwLock<BTreeMap<RequestKey, PinnedRoute>>,
    retention: Mutex<RetentionRing>,
    last_topology_generation: AtomicU64,
    invalidation_epoch: AtomicU64,
    journal_floor: AtomicU64,
    evictions: Mutex<VecDeque<EvictionIntent>>,
}

impl AssetCache {
    /// Creates a new asset cache with the given LRU retention capacity.
    ///
    /// If `retention_capacity == 0`, unpinned assets are released from the cache
    /// as soon as external consumer handles drop.
    pub fn new(retention_capacity: usize) -> Self {
        Self {
            slots: RwLock::new(BTreeMap::new()),
            in_flight: RwLock::new(BTreeMap::new()),
            canonical_in_flight: RwLock::new(BTreeMap::new()),
            router: RwLock::new(BTreeMap::new()),
            pinned: RwLock::new(BTreeMap::new()),
            pinned_requests: RwLock::new(BTreeMap::new()),
            retention: Mutex::new(RetentionRing::new(retention_capacity)),
            last_topology_generation: AtomicU64::new(0),
            invalidation_epoch: AtomicU64::new(0),
            journal_floor: AtomicU64::new(0),
            evictions: Mutex::new(VecDeque::new()),
        }
    }

    /// Loads an asset or returns an existing cached `Arc<T>`.
    ///
    /// Concurrent requests for the same asset will coalesce onto a single loader,
    /// sharing both successful results and errors without duplicate I/O or decoding.
    pub fn load<T: Asset>(&self, vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<Arc<T>> {
        let vpath = VfsPath::new(path.as_ref())?;
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath.clone(),
        };

        // 1. Fast path: check routed entry if topology is current
        let current_gen = vfs.topology_generation();
        {
            let router = self.router.read().unwrap_or_else(|p| p.into_inner());
            if let Some(route) = router.get(&request_key) {
                if route.generation == current_gen {
                    let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
                    if let Some(slot) = slots.get(&route.canonical) {
                        if slot.mount.instance_id == route.mount.instance_id {
                            if let Some(any_arc) = slot.upgrade() {
                                self.touch_retention_if_unpinned(
                                    &route.canonical,
                                    any_arc.clone(),
                                    &route.mount,
                                );
                                if let Ok(typed) = any_arc.downcast::<T>() {
                                    return Ok(typed);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. In-flight barrier coordination for this RequestKey
        let request_barrier = {
            let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());

            #[cfg(test)]
            test_hooks::check(
                test_hooks::Site::LoadStep2HoldingInFlight,
                self as *const Self as usize,
            );

            // Drop a stale route and its weak slot before re-checking. This also
            // covers callers that did not call `sync_topology` after a same-id
            // remount: the old CanonicalKey would otherwise let `pre_decoded`
            // adopt a consumer-held Arc from the previous mount instance.
            let stale_route = {
                let router = self.router.read().unwrap_or_else(|p| p.into_inner());
                router
                    .get(&request_key)
                    .filter(|route| route.generation != current_gen)
                    .map(|route| (route.canonical.clone(), route.mount.clone()))
            };
            if let Some((canonical, stale_mount)) = stale_route {
                self.router
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&request_key);
                // If the mount instance is still current, its bytes are still
                // valid: keep the slot for a later pre-decode hit. Only a
                // remount/removal makes the old slot unsafe.
                let instance_still_current =
                    vfs.mount_instance_id(&canonical.mount_id) == Some(stale_mount.instance_id);
                if !instance_still_current {
                    self.slots
                        .write()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&canonical);
                    self.retention
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&canonical);
                }
            }

            // Re-check fast path under lock in case another thread just completed
            {
                let router = self.router.read().unwrap_or_else(|p| p.into_inner());
                if let Some(route) = router.get(&request_key) {
                    if route.generation == current_gen {
                        let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
                        if let Some(slot) = slots.get(&route.canonical) {
                            if slot.mount.instance_id == route.mount.instance_id {
                                if let Some(any_arc) = slot.upgrade() {
                                    self.touch_retention_if_unpinned(
                                        &route.canonical,
                                        any_arc.clone(),
                                        &route.mount,
                                    );
                                    if let Ok(typed) = any_arc.downcast::<T>() {
                                        return Ok(typed);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // A caller that starts after an invalidation must not coalesce onto a
            // decoder that started before it: that decoder may still return
            // pre-invalidation bytes. Supersede its map entry instead.
            let latest_epoch = self.invalidation_epoch.load(Ordering::Acquire);
            if let Some(existing) = in_flight.get(&request_key) {
                if existing.start_seq >= latest_epoch {
                    let existing_barrier = existing.barrier.clone();
                    drop(in_flight);
                    match existing_barrier.wait_result() {
                        Ok(any_arc) => {
                            let typed =
                                any_arc.downcast::<T>().map_err(|_| AssetError::Decode {
                                    path: vpath.clone(),
                                    reason: "type downcast failed for shared asset".into(),
                                })?;
                            return Ok(typed);
                        }
                        Err(shared_err) => {
                            return Err(AssetError::Shared(shared_err));
                        }
                    }
                }
            }

            let new_barrier = Arc::new(InFlightBarrier::new());
            in_flight.insert(
                request_key.clone(),
                InFlightEntry {
                    barrier: new_barrier.clone(),
                    start_seq: latest_epoch,
                },
            );
            new_barrier
        };

        // 3. Executing loader guard (panic and error recovery)
        let mut guard = LoadingGuard {
            cache: self,
            request_key: request_key.clone(),
            canonical_key: None,
            request_barrier: request_barrier.clone(),
            canonical_barrier: None,
            success: false,
        };

        let read_output = match vfs.read_with_provenance(&vpath) {
            Ok(output) => output,
            Err(vfs_err) => {
                let err = Arc::new(AssetError::Vfs(vfs_err));
                request_barrier.publish(Err(err.clone()));
                self.remove_in_flight_if_matching(&request_key, &request_barrier);
                guard.success = true;
                return Err(AssetError::Shared(err));
            }
        };

        let mount_stamp = mount_stamp_for(vfs, &read_output.mount_id);

        let canonical_key = CanonicalKey {
            type_id: TypeId::of::<T>(),
            mount_id: read_output.mount_id,
            canonical_relative_path: read_output.canonical_relative_path,
        };
        guard.canonical_key = Some(canonical_key.clone());

        // 4. Canonical decode barrier coordination:
        // Prevents parallel decoding if two concurrent requests used different casings (e.g. /bf.png and /BF.PNG)
        // The `slots` read guard is released before any other cache lock is
        // taken. The cache lock order is:
        //   in_flight -> router -> slots -> pinned -> retention
        let pre_decoded = {
            let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
            match mount_stamp.as_ref() {
                Some(mount) => slots
                    .get(&canonical_key)
                    .filter(|slot| slot.mount.instance_id == mount.instance_id)
                    .and_then(SlotEntry::upgrade),
                None => None,
            }
        };

        #[cfg(test)]
        test_hooks::check(
            test_hooks::Site::LoadStep4BeforeInstall,
            self as *const Self as usize,
        );

        let any_arc = match pre_decoded {
            // Already decoded by a previous operation.
            Some(arc) => arc,
            None => {
                let (canonical_barrier, need_decode) = {
                    let mut can_in_flight = self
                        .canonical_in_flight
                        .write()
                        .unwrap_or_else(|p| p.into_inner());

                    if let Some(existing) = can_in_flight.get(&canonical_key).cloned() {
                        (existing, false)
                    } else {
                        let new_barrier = Arc::new(InFlightBarrier::new());
                        can_in_flight.insert(canonical_key.clone(), new_barrier.clone());
                        (new_barrier, true)
                    }
                };

                if !need_decode {
                    // Another thread is currently decoding this exact canonical key;
                    // wait for it rather than decoding a second time.
                    match canonical_barrier.wait_result() {
                        Ok(arc) => arc,
                        Err(err) => {
                            request_barrier.publish(Err(err.clone()));
                            self.remove_in_flight_if_matching(&request_key, &request_barrier);
                            guard.success = true;
                            return Err(AssetError::Shared(err));
                        }
                    }
                } else {
                    guard.canonical_barrier = Some(canonical_barrier.clone());
                    match T::decode(&read_output.bytes, &vpath) {
                        Ok(decoded) => {
                            let any_arc_new: Arc<dyn Any + Send + Sync> = Arc::new(decoded);
                            canonical_barrier.publish(Ok(any_arc_new.clone()));
                            self.remove_canonical_in_flight_if_matching(
                                &canonical_key,
                                &canonical_barrier,
                            );
                            any_arc_new
                        }
                        Err(decode_err) => {
                            let err = Arc::new(AssetError::Decode {
                                path: vpath,
                                reason: decode_err.to_string(),
                            });
                            canonical_barrier.publish(Err(err.clone()));
                            self.remove_canonical_in_flight_if_matching(
                                &canonical_key,
                                &canonical_barrier,
                            );
                            request_barrier.publish(Err(err.clone()));
                            self.remove_in_flight_if_matching(&request_key, &request_barrier);
                            guard.success = true;
                            return Err(AssetError::Shared(err));
                        }
                    }
                }
            }
        };

        // Install route + slot. Installation is skipped when a concurrent eviction
        // invalidated this request, when the topology changed after the bytes were
        // resolved, or when this load predates a recorded eviction intent. The
        // value is still returned to the caller.
        if let Some(mount) = &mount_stamp {
            self.install_if_current(
                &request_key,
                &canonical_key,
                InstallContext {
                    mount,
                    gen_used_for_bytes: current_gen,
                    current_vfs_gen: vfs.topology_generation(),
                },
                &request_barrier,
                &any_arc,
            );
        }

        // Publish success to waiting threads
        request_barrier.publish(Ok(any_arc.clone()));
        self.remove_in_flight_if_matching(&request_key, &request_barrier);
        guard.success = true;

        let typed = any_arc.downcast::<T>().map_err(|_| AssetError::Decode {
            path: vpath,
            reason: "type downcast failed for loaded asset".into(),
        })?;
        self.flush_evicted_slots();
        Ok(typed)
    }

    /// Instant in-memory lookup. Returns `None` on cache miss, route invalidation,
    /// or if the asset was evicted and all consumer handles dropped.
    ///
    /// Performs zero disk I/O and zero syscalls.
    pub fn get<T: Asset>(&self, vfs: &Vfs, path: impl AsRef<str>) -> Option<Arc<T>> {
        let vpath = VfsPath::new(path.as_ref()).ok()?;
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath,
        };

        let current_gen = vfs.topology_generation();
        let router = self.router.read().unwrap_or_else(|p| p.into_inner());
        let route = router.get(&request_key)?;
        if route.generation != current_gen {
            return None;
        }

        let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
        let slot = slots.get(&route.canonical)?;
        if slot.mount.instance_id != route.mount.instance_id {
            return None;
        }
        let any_arc = slot.upgrade()?;
        self.touch_retention_if_retained(&route.canonical);
        any_arc.downcast::<T>().ok()
    }

    /// Loads an asset and pins it permanently in memory.
    ///
    /// Pinned assets bypass the LRU retention ring and remain pinned independently
    /// of mount topology changes.
    ///
    /// Success guarantees the exact `Arc` returned by this call is present in the
    /// pinned set when the function returns. If the route was evicted, the
    /// topology changed, or the slot now holds a different handle, this returns
    /// [`AssetError::PinInvalidated`] and leaves no pinned state behind.
    ///
    /// There is no automatic retry after invalidation.
    pub fn pin<T: Asset>(&self, vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<Arc<T>> {
        let vpath = VfsPath::new(path.as_ref())?;
        let typed_arc = self.load::<T>(vfs, &vpath)?;
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath.clone(),
        };
        let any_arc: Arc<dyn Any + Send + Sync> = typed_arc.clone();

        #[cfg(test)]
        test_hooks::check(
            test_hooks::Site::PinAfterLoadBeforeVerify,
            self as *const Self as usize,
        );

        // Mandatory lock order:
        //   in_flight -> pinned_requests -> router -> slots -> pinned -> retention
        // `slots` is released before `pinned_requests`/`pinned` are mutated.
        let _in_flight = self.in_flight.read().unwrap_or_else(|p| p.into_inner());
        let mut pinned_req = self
            .pinned_requests
            .write()
            .unwrap_or_else(|p| p.into_inner());
        let router = self.router.read().unwrap_or_else(|p| p.into_inner());

        let route = match router.get(&request_key) {
            Some(route) if route.generation == vfs.topology_generation() => route,
            Some(_) => return Err(pin_invalidated(&vpath, PinInvalidation::TopologyChanged)),
            None => return Err(pin_invalidated(&vpath, PinInvalidation::RouteEvicted)),
        };

        {
            let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
            let current = slots
                .get(&route.canonical)
                .filter(|slot| slot.mount.instance_id == route.mount.instance_id)
                .and_then(SlotEntry::upgrade);
            if !current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &any_arc))
            {
                return Err(pin_invalidated(&vpath, PinInvalidation::HandleSuperseded));
            }
        }

        let canonical_key = route.canonical.clone();
        let mount = route.mount.clone();
        let previous = pinned_req.insert(
            request_key,
            PinnedRoute {
                canonical: canonical_key.clone(),
                mount: mount.clone(),
            },
        );

        let mut previous_handle = None;
        {
            let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
            pinned.insert(
                canonical_key.clone(),
                PinnedHandle {
                    handle: any_arc,
                    mount,
                },
            );
            if let Some(previous) = previous {
                if previous.canonical != canonical_key
                    && !pinned_req
                        .values()
                        .any(|route| route.canonical == previous.canonical)
                {
                    if let Some(handle) = pinned.remove(&previous.canonical) {
                        previous_handle = Some((previous.canonical, handle));
                    }
                }
            }
        }

        {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((previous_canonical, handle)) = previous_handle {
                retention.touch(previous_canonical, handle.handle, handle.mount);
            }
            retention.remove(&canonical_key);
        }

        Ok(typed_arc)
    }

    /// Removes an asset from the pinned set, returning it to the LRU retention ring.
    ///
    /// Removes every request alias that maps to the resolved canonical identity.
    /// Functions reliably even if VFS mount topology changed after pinning.
    pub fn unpin<T: Asset>(&self, path: impl AsRef<str>) {
        let Ok(vpath) = VfsPath::new(path.as_ref()) else {
            return;
        };
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath,
        };

        let canonical_key = {
            let mut pinned_req = self
                .pinned_requests
                .write()
                .unwrap_or_else(|p| p.into_inner());
            let direct = pinned_req.remove(&request_key).map(|route| route.canonical);
            let canonical = direct.or_else(|| {
                let router = self.router.read().unwrap_or_else(|p| p.into_inner());
                router
                    .get(&request_key)
                    .map(|route| route.canonical.clone())
            });
            if let Some(canonical) = &canonical {
                let aliases: Vec<RequestKey> = pinned_req
                    .iter()
                    .filter(|(_, route)| route.canonical == *canonical)
                    .map(|(request, _)| request.clone())
                    .collect();
                for alias in aliases {
                    pinned_req.remove(&alias);
                }
            }
            canonical
        };

        if let Some(can_key) = canonical_key {
            let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
            if let Some(handle) = pinned.remove(&can_key) {
                let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
                retention.touch(can_key, handle.handle, handle.mount);
            }
        }
    }

    /// Checks if an asset is currently pinned.
    ///
    /// Reports correct state even across mount topology changes.
    pub fn is_pinned<T: Asset>(&self, path: impl AsRef<str>) -> bool {
        let Ok(vpath) = VfsPath::new(path.as_ref()) else {
            return false;
        };
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath,
        };

        let pinned_req = self
            .pinned_requests
            .read()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(route) = pinned_req.get(&request_key) {
            let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
            return pinned.contains_key(&route.canonical);
        }

        let router = self.router.read().unwrap_or_else(|p| p.into_inner());
        if let Some(route) = router.get(&request_key) {
            let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
            pinned.contains_key(&route.canonical)
        } else {
            false
        }
    }

    /// Evicts a typed asset from active slots, routing tables, pinned stores, and the retention ring.
    ///
    /// This is the legacy exact-request operation: it matches only the exact
    /// request spelling. It does not resolve the governing mount and does not
    /// case-fold aliases. It also does not participate in the eviction epoch or
    /// intent journal, so a differently-cased in-flight load can still install
    /// after it. Use [`AssetCache::evict_path_case_aware`] after writes that may
    /// affect differently-cased paths on case-insensitive mounts.
    pub fn evict<T: Asset>(&self, path: impl AsRef<str>) {
        let Ok(vpath) = VfsPath::new(path.as_ref()) else {
            return;
        };
        let request_key = RequestKey {
            type_id: TypeId::of::<T>(),
            path: vpath,
        };

        let mut canonical_to_evict = None;

        // Tier 1: see `install_if_current`. Holding `in_flight` write for the whole
        // eviction makes "install" and "evict" mutually exclusive (I1/I2).
        let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());
        in_flight.remove(&request_key);
        self.canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        {
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            if let Some(entry) = router.remove(&request_key) {
                canonical_to_evict = Some(entry.canonical);
            }
        }
        {
            let mut pinned_req = self
                .pinned_requests
                .write()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(route) = pinned_req.remove(&request_key) {
                canonical_to_evict = Some(route.canonical);
            }
        }

        if let Some(can_key) = canonical_to_evict {
            {
                let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
                slots.remove(&can_key);
            }
            {
                let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
                pinned.remove(&can_key);
            }
            {
                let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
                retention.remove(&can_key);
            }
        }
        drop(in_flight);
    }

    /// Evicts all cached entries whose virtual path matches or starts with the given prefix.
    ///
    /// This is the legacy exact-case operation. It compares virtual path
    /// components exactly, performs no case folding, and does not resolve mount
    /// provenance. It also does not participate in the eviction epoch or intent
    /// journal, so a differently-cased in-flight load can still install after it.
    /// Use [`AssetCache::evict_path_case_aware`] for the
    /// `vfs.write` -> eviction workflow on case-insensitive mounts.
    pub fn evict_path(&self, path: impl AsRef<str>) {
        let Ok(vpath) = VfsPath::new(path.as_ref()) else {
            return;
        };

        #[cfg(test)]
        test_hooks::check(
            test_hooks::Site::EvictPathEntry,
            self as *const Self as usize,
        );

        // Tier 1: see `install_if_current`. Removing the matching barriers while
        // holding `in_flight` write guarantees that every load which could still
        // install this subtree either installs before us (and is removed below)
        // or observes its barrier gone and declines to install (I1/I2).
        let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());
        in_flight.retain(|req, _| !(req.path == vpath || req.path.starts_with(&vpath)));
        // Canonical decode barriers cannot be scoped by request path, so they are
        // dropped wholesale: this only disables decode coalescing for the window,
        // it never installs anything.
        self.canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        let mut affected_canonical = BTreeSet::new();

        {
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            let to_remove: Vec<RequestKey> = router
                .keys()
                .filter(|req| req.path == vpath || req.path.starts_with(&vpath))
                .cloned()
                .collect();
            for key in to_remove {
                if let Some(entry) = router.remove(&key) {
                    affected_canonical.insert(entry.canonical);
                }
            }
        }
        {
            let mut pinned_req = self
                .pinned_requests
                .write()
                .unwrap_or_else(|p| p.into_inner());
            let to_remove: Vec<RequestKey> = pinned_req
                .keys()
                .filter(|req| req.path == vpath || req.path.starts_with(&vpath))
                .cloned()
                .collect();
            for key in to_remove {
                if let Some(route) = pinned_req.remove(&key) {
                    affected_canonical.insert(route.canonical);
                }
            }
        }

        {
            let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
            let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());

            for canonical in affected_canonical {
                slots.remove(&canonical);
                pinned.remove(&canonical);
                retention.remove(&canonical);
            }
        }
        drop(in_flight);
        self.flush_evicted_slots();
    }

    /// Evicts a subtree using the governing write mount and its case rule.
    ///
    /// Intended for the `vfs.write(path, ..)` then
    /// `cache.evict_path_case_aware(&vfs, path)` workflow. Unlike
    /// [`AssetCache::evict_path`], this resolves the write target through the VFS
    /// and invalidates differently-cased aliases on case-insensitive mounts while
    /// preserving distinct paths on case-sensitive mounts.
    ///
    /// Prefer [`AssetCache::write_and_evict_path_case_aware`] when the write and
    /// eviction are a pair: separate calls cannot detect a topology change between
    /// `vfs.write` and this method, while the combined helper keeps both operations
    /// under one `&Vfs` borrow.
    ///
    /// The snapshot generation is revalidated before any cache mutation. If it
    /// changes, the snapshot is re-resolved; if no stable snapshot can be obtained,
    /// [`AssetError::TopologyChanged`] is returned and no cache state is changed.
    pub fn evict_path_case_aware(&self, vfs: &Vfs, path: impl AsRef<str>) -> AssetResult<()> {
        let target = VfsPath::new(path.as_ref())?;
        let snapshot = vfs.write_target(&target)?;
        self.evict_path_with_snapshot(vfs, target, snapshot, MAX_SNAPSHOT_ATTEMPTS)
    }

    /// Writes bytes through the VFS and invalidates the corresponding cache
    /// subtree under one immutable VFS borrow.
    ///
    /// This is the preferred write -> invalidate workflow. Because both the
    /// write and the eviction happen while `&Vfs` is held, safe Rust prevents a
    /// mount/unmount/enable/disable from changing the governing mount between
    /// them. The snapshot is taken before the write and revalidated before any
    /// cache mutation. If the write fails, nothing is evicted.
    pub fn write_and_evict_path_case_aware(
        &self,
        vfs: &Vfs,
        path: impl AsRef<str>,
        data: &[u8],
    ) -> AssetResult<()> {
        let target = VfsPath::new(path.as_ref())?;
        let snapshot = vfs.write_target(&target)?;
        vfs.write(&target, data)?;
        self.evict_path_with_snapshot(vfs, target, snapshot, MAX_SNAPSHOT_ATTEMPTS)
    }

    /// Testable core of [`AssetCache::evict_path_case_aware`].
    ///
    /// `max_attempts` bounds re-resolutions of a stale snapshot; zero attempts
    /// means any generation mismatch fails before touching cache state.
    fn evict_path_with_snapshot(
        &self,
        vfs: &Vfs,
        target: VfsPath,
        mut snapshot: VfsWriteTarget,
        max_attempts: usize,
    ) -> AssetResult<()> {
        let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());

        let mut attempts = 0usize;
        while snapshot.topology_generation != vfs.topology_generation() {
            if attempts >= max_attempts {
                return Err(AssetError::TopologyChanged(target));
            }
            attempts += 1;
            snapshot = vfs.write_target(&target)?;
        }

        let target_mount = mount_stamp_from_target(&snapshot);
        let seq = self.invalidation_epoch.fetch_add(1, Ordering::AcqRel) + 1;

        {
            let mut intents = self.evictions.lock().unwrap_or_else(|p| p.into_inner());
            intents.push_back(EvictionIntent {
                seq,
                topology_generation: snapshot.topology_generation,
                target: target.clone(),
                target_mount: target_mount.clone(),
            });
            while intents.len() > MAX_EVICTION_INTENTS {
                if let Some(dropped) = intents.pop_front() {
                    self.journal_floor.fetch_max(dropped.seq, Ordering::AcqRel);
                }
            }
        }

        // Exact-subtree in-flight entries are removed exactly as in the legacy
        // method. Differently-cased entries are retained and handled by the
        // generation-aware intent checked at install time.
        in_flight.retain(|request, _| !request.path.starts_with(&target));
        // Canonical decode barriers cannot be scoped by request path. Dropping
        // them wholesale prevents a post-eviction load from waiting on a
        // pre-eviction decoder; it never installs anything by itself.
        self.canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        let mut identities: BTreeSet<(String, String)> = BTreeSet::new();
        let mut routes_to_remove: Vec<RequestKey> = Vec::new();
        {
            let router = self.router.read().unwrap_or_else(|p| p.into_inner());
            for (request, route) in router.iter() {
                if entry_affected(
                    &request.path,
                    &route.canonical,
                    &route.mount,
                    &target,
                    target_mount.as_ref(),
                ) {
                    routes_to_remove.push(request.clone());
                    if identity_affected_by_entry(
                        &request.path,
                        &route.canonical,
                        &route.mount,
                        &target,
                        target_mount.as_ref(),
                    ) {
                        identities.insert(identity_of(&route.canonical));
                    }
                }
            }
        }
        {
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            for request in &routes_to_remove {
                router.remove(request);
            }
            let aliases: Vec<RequestKey> = router
                .iter()
                .filter(|(_, route)| identities.contains(&identity_of(&route.canonical)))
                .map(|(request, _)| request.clone())
                .collect();
            for request in aliases {
                router.remove(&request);
            }
        }

        // Expand affected identities from slots/pins/retention that match the
        // target mount even if they currently have no route (retention/unpin
        // windows). This prevents a fresh load from adopting an old retained slot.
        {
            let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
            for (key, slot) in slots.iter() {
                let same_mount = target_mount.as_ref().is_some_and(|target_mount| {
                    slot.mount.instance_id == target_mount.instance_id
                        && same_mount_identity_affected(key, &target, target_mount)
                });
                if same_mount || canonical_is_under_target(key, &slot.mount, &target) {
                    identities.insert(identity_of(key));
                }
            }
        }
        {
            let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
            for (key, handle) in pinned.iter() {
                let same_mount = target_mount.as_ref().is_some_and(|target_mount| {
                    handle.mount.instance_id == target_mount.instance_id
                        && same_mount_identity_affected(key, &target, target_mount)
                });
                if same_mount || canonical_is_under_target(key, &handle.mount, &target) {
                    identities.insert(identity_of(key));
                }
            }
        }
        {
            let retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            for (key, handle) in retention.retained_handles.iter() {
                let same_mount = target_mount.as_ref().is_some_and(|target_mount| {
                    handle.mount.instance_id == target_mount.instance_id
                        && same_mount_identity_affected(key, &target, target_mount)
                });
                if same_mount || canonical_is_under_target(key, &handle.mount, &target) {
                    identities.insert(identity_of(key));
                }
            }
        }

        let mut pin_request_hits: Vec<(RequestKey, CanonicalKey)> = Vec::new();
        {
            let pinned_req = self
                .pinned_requests
                .read()
                .unwrap_or_else(|p| p.into_inner());
            for (request, route) in pinned_req.iter() {
                if entry_affected(
                    &request.path,
                    &route.canonical,
                    &route.mount,
                    &target,
                    target_mount.as_ref(),
                ) {
                    if identity_affected_by_entry(
                        &request.path,
                        &route.canonical,
                        &route.mount,
                        &target,
                        target_mount.as_ref(),
                    ) {
                        identities.insert(identity_of(&route.canonical));
                    }
                    pin_request_hits.push((request.clone(), route.canonical.clone()));
                }
            }
        }

        let mut previous_handles = Vec::new();
        {
            let mut pinned_req = self
                .pinned_requests
                .write()
                .unwrap_or_else(|p| p.into_inner());
            for (request, _) in &pin_request_hits {
                pinned_req.remove(request);
            }
            let aliases: Vec<RequestKey> = pinned_req
                .iter()
                .filter(|(_, route)| identities.contains(&identity_of(&route.canonical)))
                .map(|(request, _)| request.clone())
                .collect();
            for request in aliases {
                pinned_req.remove(&request);
            }

            let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
            let keys: Vec<CanonicalKey> = pinned.keys().cloned().collect();
            for key in keys {
                let hard_hit = identities.contains(&identity_of(&key));
                let soft_hit = !hard_hit
                    && pin_request_hits
                        .iter()
                        .any(|(_, canonical)| canonical == &key)
                    && !pinned_req.values().any(|route| route.canonical == key);
                if hard_hit {
                    pinned.remove(&key);
                } else if soft_hit {
                    if let Some(handle) = pinned.remove(&key) {
                        previous_handles.push((key, handle));
                    }
                }
            }
        }

        {
            let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
            let keys: Vec<CanonicalKey> = slots
                .keys()
                .filter(|key| identities.contains(&identity_of(key)))
                .cloned()
                .collect();
            for key in keys {
                slots.remove(&key);
            }
        }

        {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            let keys: Vec<CanonicalKey> = retention
                .retained_handles
                .keys()
                .filter(|key| identities.contains(&identity_of(key)))
                .cloned()
                .collect();
            for key in keys {
                retention.remove(&key);
            }
            for (key, handle) in previous_handles {
                retention.touch(key, handle.handle, handle.mount);
            }
        }

        self.prune_eviction_intents(&in_flight);
        drop(in_flight);
        self.flush_evicted_slots();
        Ok(())
    }

    /// Invalidates active request routes, forcing subsequent lookups to re-evaluate VFS mounts.
    ///
    /// Pinned assets and reverse pin indices are preserved. Decoded bytes that
    /// were reachable only through an invalidated route are dropped from the
    /// retention ring, because the mount that produced them may no longer be the
    /// resolving mount (F4b). Live consumer handles keep their `Arc`.
    pub fn invalidate_topology(&self) {
        let _in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());
        let epoch = self.invalidation_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        // Any load that registered before this invalidation must not install
        // afterwards, even when the VFS generation itself did not change.
        self.journal_floor.fetch_max(epoch, Ordering::AcqRel);
        // Post-invalidation loads must not join a pre-invalidation decoder,
        // whether through a request barrier or a canonical decode barrier.
        self.canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        let mut stale: BTreeSet<CanonicalKey> = BTreeSet::new();
        {
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            // Set generation on all existing routes to 0 (stale), preserving keys for identity operations
            for entry in router.values_mut() {
                stale.insert(entry.canonical.clone());
                entry.generation = 0;
            }
        }
        if stale.is_empty() {
            return;
        }

        // A topology change can remount the same mount id with new content.
        // Remove the weak slot entries so a new load cannot adopt a consumer-held
        // old Arc for the same CanonicalKey. Live consumer handles remain valid.
        {
            let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
            for key in &stale {
                slots.remove(key);
            }
        }
        let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
        for key in &stale {
            retention.remove(key);
        }
    }

    /// Synchronizes cache routing with current VFS topology generation.
    pub fn sync_topology(&self, vfs: &Vfs) {
        let current = vfs.topology_generation();
        let last = self
            .last_topology_generation
            .swap(current, Ordering::AcqRel);
        if current != last {
            self.invalidate_topology();
        }
    }

    /// Completely purges all cached, pinned, and retained assets.
    ///
    /// An in-flight load that started before `clear()` returns keeps its decoded
    /// value but must not install cache state afterwards (I1/I2).
    pub fn clear(&self) {
        // Tier 1: mutually exclusive with installation (see `install_if_current`).
        let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        in_flight.clear();
        self.evictions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.journal_floor.store(0, Ordering::Release);
        self.canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        {
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            router.clear();
        }
        {
            let mut pinned_req = self
                .pinned_requests
                .write()
                .unwrap_or_else(|p| p.into_inner());
            pinned_req.clear();
        }
        {
            let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
            slots.clear();
        }
        {
            let mut pinned = self.pinned.write().unwrap_or_else(|p| p.into_inner());
            pinned.clear();
        }
        {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            retention.clear();
        }
        drop(in_flight);
    }

    /// Sets a new retention capacity. Evicts excess entries immediately if the new capacity is smaller.
    pub fn set_retention_capacity(&self, new_capacity: usize) {
        let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
        retention.set_capacity(new_capacity);
    }

    /// Returns the configured capacity of the retention ring.
    pub fn retention_capacity(&self) -> usize {
        let retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
        retention.capacity
    }

    /// Returns the number of unpinned assets currently held alive by the retention ring.
    pub fn retained_count(&self) -> usize {
        let retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
        retention.len()
    }

    /// Returns the number of assets currently pinned.
    pub fn pinned_count(&self) -> usize {
        let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
        pinned.len()
    }

    /// Returns the number of live (non-expired) decoded asset instances in the cache.
    pub fn cached_count(&self) -> usize {
        let slots = self.slots.read().unwrap_or_else(|p| p.into_inner());
        slots
            .values()
            .filter(|slot| slot.weak.strong_count() > 0)
            .count()
    }

    /// Prunes expired weak references from the cache map.
    pub fn prune_expired(&self) {
        let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
        slots.retain(|_, slot| slot.weak.strong_count() > 0);
    }

    fn remove_in_flight_if_matching(&self, key: &RequestKey, barrier: &Arc<InFlightBarrier>) {
        let mut in_flight = self.in_flight.write().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = in_flight.get(key) {
            if Arc::ptr_eq(&existing.barrier, barrier) {
                in_flight.remove(key);
            }
        }
    }

    fn remove_canonical_in_flight_if_matching(
        &self,
        key: &CanonicalKey,
        barrier: &Arc<InFlightBarrier>,
    ) {
        let mut in_flight = self
            .canonical_in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = in_flight.get(key) {
            if Arc::ptr_eq(existing, barrier) {
                in_flight.remove(key);
            }
        }
    }

    fn touch_retention_if_unpinned(
        &self,
        canonical_key: &CanonicalKey,
        handle: Arc<dyn Any + Send + Sync>,
        mount: &MountStamp,
    ) {
        let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
        if !pinned.contains_key(canonical_key) {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            retention.touch(canonical_key.clone(), handle, mount.clone());
        }
    }

    fn touch_retention_if_retained(&self, canonical_key: &CanonicalKey) {
        let pinned = self.pinned.read().unwrap_or_else(|p| p.into_inner());
        if !pinned.contains_key(canonical_key) {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            retention.touch_existing(canonical_key);
        }
    }

    /// Installs the route and slot entry for a completed load, unless the load was
    /// invalidated while it was in flight.
    ///
    /// Takes `in_flight` read for the whole installation, which is tier 1 of the
    /// cache lock order. Eviction paths take `in_flight` write before stripping
    /// barriers, so installation is atomic with respect to them: either the load
    /// installs first and the eviction removes it, or the eviction removes the
    /// barrier first and the load declines to install (I1/I2).
    ///
    /// Also declines when the topology generation the bytes were resolved under is
    /// no longer current (I3). Old bytes are never re-stamped with a newer
    /// generation.
    fn install_if_current(
        &self,
        request_key: &RequestKey,
        canonical_key: &CanonicalKey,
        context: InstallContext<'_>,
        request_barrier: &Arc<InFlightBarrier>,
        any_arc: &Arc<dyn Any + Send + Sync>,
    ) -> bool {
        let in_flight = self.in_flight.read().unwrap_or_else(|p| p.into_inner());

        let Some(entry) = in_flight.get(request_key) else {
            return false;
        };
        if !Arc::ptr_eq(&entry.barrier, request_barrier)
            || context.gen_used_for_bytes != context.current_vfs_gen
        {
            return false;
        }

        if entry.start_seq < self.journal_floor.load(Ordering::Acquire) {
            return false;
        }

        {
            let intents = self.evictions.lock().unwrap_or_else(|p| p.into_inner());
            for intent in intents.iter().rev() {
                if intent.seq <= entry.start_seq {
                    break;
                }
                if intent_affects(
                    intent,
                    &request_key.path,
                    canonical_key,
                    context.mount,
                    context.gen_used_for_bytes,
                ) {
                    return false;
                }
            }
        }

        {
            // Forward order: router -> slots, so a concurrent `get` can never
            // observe a route that points at a slot it cannot upgrade.
            let mut router = self.router.write().unwrap_or_else(|p| p.into_inner());
            router.insert(
                request_key.clone(),
                RouteEntry {
                    canonical: canonical_key.clone(),
                    generation: context.gen_used_for_bytes,
                    mount: context.mount.clone(),
                },
            );
            let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
            let reusable = slots
                .get(canonical_key)
                .filter(|slot| slot.mount.instance_id == context.mount.instance_id)
                .and_then(SlotEntry::upgrade)
                .is_some();
            if !reusable {
                slots.insert(
                    canonical_key.clone(),
                    SlotEntry::new(any_arc, context.mount),
                );
            }
        }

        drop(in_flight);
        self.touch_retention_if_unpinned(canonical_key, any_arc.clone(), context.mount);
        true
    }

    /// Drops eviction intents that no registered in-flight load can still need.
    ///
    /// Must be called while holding `in_flight` write. A load not present in the
    /// map cannot install (its barrier check fails), so only the minimum
    /// `start_seq` of remaining entries matters.
    fn prune_eviction_intents(&self, in_flight: &BTreeMap<RequestKey, InFlightEntry>) {
        let min_start = in_flight.values().map(|entry| entry.start_seq).min();
        let mut intents = self.evictions.lock().unwrap_or_else(|p| p.into_inner());
        match min_start {
            Some(min_start) => {
                while intents
                    .front()
                    .is_some_and(|intent| intent.seq <= min_start)
                {
                    intents.pop_front();
                }
            }
            None => intents.clear(),
        }
    }

    /// Prunes `slots` entries whose strong handle was released by retention
    /// eviction. Called with no cache locks held; the ring records the evicted
    /// keys so this stays O(evicted) instead of O(slots) on the hot path.
    fn flush_evicted_slots(&self) {
        let dead = {
            let mut retention = self.retention.lock().unwrap_or_else(|p| p.into_inner());
            if retention.dead_keys.is_empty() {
                return;
            }
            std::mem::take(&mut retention.dead_keys)
        };
        let mut slots = self.slots.write().unwrap_or_else(|p| p.into_inner());
        for key in dead {
            let prune = match slots.get(&key) {
                Some(slot) => slot.upgrade().is_none(),
                None => true,
            };
            if prune {
                slots.remove(&key);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Concurrency regression tests (F1-F5).
//
// These live inside `mod cache` so they can observe cache internals (slot map,
// retention ring, route table) without widening the public API. They are gated
// on `cfg(test)` and never compiled into release builds.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod concurrency_tests {
    use std::any::{Any, TypeId};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Condvar, LazyLock, Mutex};
    use std::time::{Duration, Instant};

    use fu_vfs::{CaseSensitivity, MemoryMount, Mount, MountPolicy};

    use super::test_hooks::{self, Site};
    use super::{
        intent_affects, AssetCache, CanonicalKey, EvictionIntent, InFlightBarrier, InFlightEntry,
        MountStamp, RequestKey, RouteEntry, SlotEntry, MAX_SNAPSHOT_ATTEMPTS,
    };
    use crate::asset::Asset;
    use fu_vfs::{Vfs, VfsPath};

    /// Budget for a single cooperative rendezvous (gate reached / released).
    const GATE_WAIT: Duration = Duration::from_secs(10);
    /// Budget for expecting a worker to finish; exceeding it is a deadlock failure.
    const DEADLOCK_WAIT: Duration = Duration::from_secs(3);

    // ---------------------------------------------------------------- harness

    struct GateState {
        entered: bool,
        released: bool,
    }

    /// A channel-free rendezvous usable from a static `Asset::decode`.
    struct Gate {
        state: Mutex<GateState>,
        cv: Condvar,
    }

    impl Gate {
        fn new() -> Self {
            Self {
                state: Mutex::new(GateState {
                    entered: false,
                    released: false,
                }),
                cv: Condvar::new(),
            }
        }

        /// Blocks the decoding thread until the test releases it.
        fn block_until_released(&self) {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.entered = true;
            self.cv.notify_all();
            let start = Instant::now();
            while !st.released && start.elapsed() < GATE_WAIT {
                let (next, _) = self
                    .cv
                    .wait_timeout(st, Duration::from_millis(20))
                    .unwrap_or_else(|p| p.into_inner());
                st = next;
            }
        }

        fn wait_entered(&self, timeout: Duration) -> bool {
            let start = Instant::now();
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            while !st.entered {
                let left = timeout.saturating_sub(start.elapsed());
                if left.is_zero() {
                    return false;
                }
                let (next, _) = self
                    .cv
                    .wait_timeout(st, left)
                    .unwrap_or_else(|p| p.into_inner());
                st = next;
            }
            true
        }

        fn release(&self) {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.released = true;
            self.cv.notify_all();
        }
    }

    static GATES: LazyLock<Mutex<BTreeMap<String, Arc<Gate>>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    static DECODE_CALLS: LazyLock<Mutex<BTreeMap<String, usize>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    /// Serializes tests that share the global `PinAfterLoadBeforeVerify` site.
    static PIN_HOOK_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn install_gate(path: &str) -> Arc<Gate> {
        let gate = Arc::new(Gate::new());
        GATES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(path.to_string(), gate.clone());
        gate
    }

    fn remove_gate(path: &str) {
        GATES.lock().unwrap_or_else(|p| p.into_inner()).remove(path);
    }

    fn decode_calls(path: &str) -> usize {
        DECODE_CALLS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct GatedAsset {
        bytes: Vec<u8>,
    }

    impl Asset for GatedAsset {
        type Error = std::convert::Infallible;

        fn decode(bytes: &[u8], path: &VfsPath) -> Result<Self, Self::Error> {
            let key = path.as_str().to_string();
            *DECODE_CALLS
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entry(key.clone())
                .or_insert(0) += 1;
            let gate = GATES
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&key)
                .cloned();
            if let Some(gate) = gate {
                gate.block_until_released();
            }
            Ok(GatedAsset {
                bytes: bytes.to_vec(),
            })
        }
    }

    fn memory_vfs(files: &[(&str, &[u8])]) -> (Vfs, Arc<MemoryMount>) {
        memory_vfs_with_case(files, CaseSensitivity::Sensitive)
    }

    fn memory_vfs_with_case(
        files: &[(&str, &[u8])],
        case: CaseSensitivity,
    ) -> (Vfs, Arc<MemoryMount>) {
        let mut vfs = Vfs::new();
        let mem = Arc::new(MemoryMount::new(case));
        for (path, bytes) in files {
            mem.set_file(path, *bytes).unwrap();
        }
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            case,
            mem.clone(),
        ))
        .unwrap();
        (vfs, mem)
    }

    /// A spawned worker whose completion can be observed without joining.
    struct Task<T> {
        rx: mpsc::Receiver<T>,
        done: Arc<AtomicBool>,
    }

    impl<T> Task<T> {
        fn is_done(&self) -> bool {
            self.done.load(Ordering::SeqCst)
        }

        fn expect(self, what: &str) -> T {
            match self.rx.recv_timeout(DEADLOCK_WAIT) {
                Ok(value) => value,
                Err(_) => panic!(
                    "{what} did not finish within {DEADLOCK_WAIT:?}: \
                     possible lock-order deadlock or lost wakeup"
                ),
            }
        }
    }

    fn spawn_task<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Task<T> {
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(AtomicBool::new(false));
        let done_flag = done.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            done_flag.store(true, Ordering::SeqCst);
        });
        Task { rx, done }
    }

    /// Polls a real observable condition (lock state / completion flag) until it
    /// holds or the budget expires. This is not a fixed sleep: the condition is
    /// the thing under test.
    fn wait_until(budget: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < budget {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        cond()
    }

    /// Disarms every hook site on drop so a failing test cannot leave other
    /// tests parked on a stale hook.
    struct HookGuard {
        sites: Vec<Site>,
    }

    impl HookGuard {
        fn new(sites: &[Site]) -> Self {
            Self {
                sites: sites.to_vec(),
            }
        }
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            for site in &self.sites {
                // `release` both stops trapping new arrivals and frees any thread
                // still parked at this site.
                test_hooks::release(*site);
            }
        }
    }

    // ------------------------------------------------------------------ F1

    /// Reproduces the exact lock-order cycle of `load` step 2 / step 4 and
    /// `evict_path`, using hooks instead of timing:
    ///
    ///   S (step 4) holds `slots` read, then wants `router` write
    ///   L (step 2) holds `in_flight` write, then wants `router` read + `slots` read
    ///   E (evict)  is queued for `slots` write
    ///
    /// Before the fix this wedges permanently; after the fix all three finish.
    #[test]
    fn f1_lock_order_cycle_must_not_deadlock() {
        let (vfs, _) = memory_vfs(&[("s/s.bin", b"s-bytes"), ("l/l.bin", b"l-bytes")]);
        let vfs = Arc::new(vfs);
        let cache = Arc::new(AssetCache::new(0));

        // L's request needs a VALID route with a DEAD slot, so that its step-2
        // re-check enters the routed branch and takes `slots` read. With capacity
        // 0 no strong reference is retained, so dropping the caller's Arc kills
        // the slot while the route survives.
        let l_warm = cache.load::<GatedAsset>(&vfs, "/l/l.bin").unwrap();
        drop(l_warm);

        // S's request needs a LIVE canonical slot with a STALE route, so it
        // re-resolves and parks in step 4 still holding `slots` read.
        let s_alive = cache.load::<GatedAsset>(&vfs, "/s/s.bin").unwrap();
        assert_eq!(s_alive.bytes, b"s-bytes".to_vec());

        // Mark routes stale without invoking `invalidate_topology`: the latter
        // now correctly drops weak slots, and this test specifically needs S's
        // externally held slot to remain live under a stale route.
        {
            let mut router = cache.router.write().unwrap_or_else(|p| p.into_inner());
            for entry in router.values_mut() {
                entry.generation = 0;
            }
        }

        // Restore a valid route for L (its slot stays dead: capacity 0).
        let l_again = cache.load::<GatedAsset>(&vfs, "/l/l.bin").unwrap();
        drop(l_again);
        assert_eq!(
            cache.cached_count(),
            1,
            "only S's externally held slot must be live"
        );

        let owner = Arc::as_ptr(&cache) as usize;
        let guard = HookGuard::new(&[
            Site::LoadStep2HoldingInFlight,
            Site::LoadStep4BeforeInstall,
            Site::EvictPathEntry,
        ]);

        // Hooks are armed one at a time, just before the thread that must park on
        // them, so an earlier stage of the same load cannot be trapped by a later
        // stage's hook.

        // S: step-4 loader (holds slots read).
        test_hooks::arm(Site::LoadStep4BeforeInstall, owner);
        let s_task = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || {
                c.load::<GatedAsset>(&v, "/s/s.bin")
                    .map(|a| a.bytes.clone())
            })
        };
        assert!(
            test_hooks::wait_reached(Site::LoadStep4BeforeInstall, GATE_WAIT),
            "step-4 loader never reached its hook"
        );
        // Keep S parked, but stop trapping the next thread through this site.
        test_hooks::disarm(Site::LoadStep4BeforeInstall);

        // L: step-2 loader (holds in_flight write).
        test_hooks::arm(Site::LoadStep2HoldingInFlight, owner);
        let l_task = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || {
                c.load::<GatedAsset>(&v, "/l/l.bin")
                    .map(|a| a.bytes.clone())
            })
        };
        assert!(
            test_hooks::wait_reached(Site::LoadStep2HoldingInFlight, GATE_WAIT),
            "step-2 loader never reached its hook"
        );
        test_hooks::disarm(Site::LoadStep2HoldingInFlight);

        // E: evictor, parked immediately before slots write.
        test_hooks::arm(Site::EvictPathEntry, owner);
        let e_task = {
            let c = cache.clone();
            spawn_task(move || c.evict_path("/zzz"))
        };
        assert!(
            test_hooks::wait_reached(Site::EvictPathEntry, GATE_WAIT),
            "evictor never reached its hook"
        );
        test_hooks::disarm(Site::EvictPathEntry);

        // Release the evictor and wait until it is genuinely parked in the slots
        // writer queue (try_read is refused only when a writer is queued).
        test_hooks::release(Site::EvictPathEntry);
        // Pre-fix the evictor immediately queues on `slots`; after the fix it may
        // first serialize on `in_flight` held by the step-2 loader. Either way it
        // gets a bounded opportunity to reach the queue; the final completion
        // assertion below is what actually detects the deadlock.
        let _ = wait_until(Duration::from_secs(1), || {
            e_task.is_done() || cache.slots.try_read().is_err()
        });

        // Release the step-2 loader and wait until it holds the router read lock.
        // After the fix it no longer parks on `slots`, so completion also counts.
        test_hooks::release(Site::LoadStep2HoldingInFlight);
        let l_holds_router = wait_until(GATE_WAIT, || {
            l_task.is_done() || cache.router.try_write().is_err()
        });
        assert!(
            l_holds_router,
            "step-2 loader neither acquired router read nor finished"
        );

        // Release the step-4 loader. Pre-fix this takes `router` write while still
        // holding `slots` read, which closes the cycle with the queued evictor.
        test_hooks::release(Site::LoadStep4BeforeInstall);

        let s = s_task.expect("step-4 loader");
        let l = l_task.expect("step-2 loader");
        e_task.expect("evictor");
        assert!(s.is_ok(), "step-4 loader failed: {s:?}");
        assert!(l.is_ok(), "step-2 loader failed: {l:?}");
        drop(guard);
    }

    /// Coarse stress companion to the deterministic test: loaders racing an
    /// evictor and a clearer must always finish.
    #[test]
    fn f1_stress_concurrent_load_evict_clear_completes() {
        let (vfs, _) = memory_vfs(&[
            ("a.bin", b"a"),
            ("b.bin", b"b"),
            ("c.bin", b"c"),
            ("d.bin", b"d"),
        ]);
        let vfs = Arc::new(vfs);
        let cache = Arc::new(AssetCache::new(4));

        let (done_tx, done_rx) = mpsc::channel::<()>();
        let paths = ["/a.bin", "/b.bin", "/c.bin", "/d.bin"].map(str::to_string);
        let workers = paths.len() + 1;

        for (index, path) in paths.into_iter().enumerate() {
            let (c, v, tx) = (cache.clone(), vfs.clone(), done_tx.clone());
            std::thread::spawn(move || {
                for _ in 0..100 {
                    let _ = c.load::<Vec<u8>>(&v, &path);
                    if index % 2 == 0 {
                        c.evict_path(&path);
                    }
                }
                let _ = tx.send(());
            });
        }
        {
            let (c, tx) = (cache.clone(), done_tx.clone());
            std::thread::spawn(move || {
                for _ in 0..100 {
                    c.evict_path("/");
                    c.clear();
                }
                let _ = tx.send(());
            });
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut finished = 0;
        while finished < workers {
            let left = deadline.saturating_duration_since(Instant::now());
            if done_rx.recv_timeout(left).is_err() {
                break;
            }
            finished += 1;
        }
        assert_eq!(
            finished, workers,
            "concurrent load/evict/clear stress did not finish: possible lock-order deadlock"
        );
    }

    // ------------------------------------------------------------------ F2

    /// A route must not be installed when the topology generation it was
    /// resolved under no longer matches the current generation. Otherwise old
    /// bytes become reachable as if they came from the new topology.
    #[test]
    fn f2_route_install_rejects_stale_generation() {
        let (vfs, _) = memory_vfs(&[("x.bin", b"x")]);
        let cache = AssetCache::new(8);
        assert_eq!(vfs.topology_generation(), 1, "one mount => generation 1");

        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/x.bin").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "x.bin".to_string(),
        };
        let handle: Arc<dyn Any + Send + Sync> = Arc::new(GatedAsset {
            bytes: b"x".to_vec(),
        });

        // Register the request as in flight so the installation path is willing to
        // run, then hand it bytes resolved under generation 0 (which predates the
        // mount and therefore cannot be current).
        let barrier = Arc::new(InFlightBarrier::new());
        cache
            .in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                request.clone(),
                InFlightEntry {
                    barrier: barrier.clone(),
                    start_seq: 0,
                },
            );

        let mount = MountStamp {
            id: "base".to_string(),
            instance_id: 1,
            mount_point: VfsPath::root(),
            priority: 0,
            case_sensitivity: CaseSensitivity::Sensitive,
        };
        let installed = cache.install_if_current(
            &request,
            &canonical,
            super::InstallContext {
                mount: &mount,
                gen_used_for_bytes: 0,
                current_vfs_gen: vfs.topology_generation(),
            },
            &barrier,
            &handle,
        );
        assert!(
            !installed,
            "installation must be refused for stale-topology bytes"
        );

        assert!(
            cache
                .router
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "a route was installed for bytes resolved under a stale topology generation"
        );
        assert_eq!(
            cache.retained_count(),
            0,
            "stale-topology bytes were retained instead of discarded"
        );
    }

    // ------------------------------------------------------------------ F3

    #[test]
    fn f3a_evict_path_during_inflight_load_installs_nothing() {
        let (vfs, _) = memory_vfs(&[("mods/song/note.bin", b"old")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/mods/song/note.bin");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || {
                c.load::<GatedAsset>(&v, "/mods/song/note.bin")
                    .map(|a| a.bytes.clone())
            })
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        // Content changes and the subtree is evicted while the load is in flight.
        vfs.write("/mods/song/note.bin", b"new").unwrap();
        cache.evict_path("/mods/song");

        gate.release();
        let loaded = loader
            .expect("in-flight loader")
            .expect("load must still return");
        assert_eq!(
            loaded,
            b"old".to_vec(),
            "the racing load still returns the bytes it decoded"
        );

        assert!(
            cache
                .get::<GatedAsset>(&vfs, "/mods/song/note.bin")
                .is_none(),
            "a load that started before evict_path must not install cache state"
        );

        let fresh = cache
            .load::<GatedAsset>(&vfs, "/mods/song/note.bin")
            .expect("fresh load");
        assert_eq!(
            fresh.bytes,
            b"new".to_vec(),
            "fresh load must see new content"
        );
        remove_gate("/mods/song/note.bin");
    }

    #[test]
    fn f3b_clear_during_inflight_load_installs_nothing() {
        let (vfs, _) = memory_vfs(&[("hud/hp.bin", b"old")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/hud/hp.bin");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || {
                c.load::<GatedAsset>(&v, "/hud/hp.bin")
                    .map(|a| a.bytes.clone())
            })
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        cache.clear();
        gate.release();
        let loaded = loader
            .expect("in-flight loader")
            .expect("load must still return");
        assert_eq!(loaded, b"old".to_vec());

        assert!(
            cache.get::<GatedAsset>(&vfs, "/hud/hp.bin").is_none(),
            "a load that started before clear() must not install cache state"
        );
        assert_eq!(cache.pinned_count(), 0);
        assert_eq!(cache.retained_count(), 0);
        remove_gate("/hud/hp.bin");
    }

    /// Scoped invalidation: evicting one subtree must not discard an unrelated
    /// in-flight load.
    #[test]
    fn f3c_evict_path_does_not_invalidate_unrelated_inflight_load() {
        let (vfs, _) = memory_vfs(&[("a/x.bin", b"a"), ("b/y.bin", b"b")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/b/y.bin");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || {
                c.load::<GatedAsset>(&v, "/b/y.bin")
                    .map(|a| a.bytes.clone())
            })
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        // Unrelated subtree is written and evicted.
        vfs.write("/a/x.bin", b"a2").unwrap();
        cache.evict_path("/a");

        gate.release();
        let loaded = loader
            .expect("in-flight loader")
            .expect("load must succeed");
        assert_eq!(loaded, b"b".to_vec());

        assert!(
            cache.get::<GatedAsset>(&vfs, "/b/y.bin").is_some(),
            "evict_path on an unrelated subtree must not discard this load"
        );
        remove_gate("/b/y.bin");
    }

    // ------------------------------------------------------------------ F4

    #[test]
    fn f4a_retention_eviction_prunes_dead_slots() {
        let (vfs, _) = memory_vfs(&[("a.bin", b"a"), ("b.bin", b"b"), ("c.bin", b"c")]);
        let cache = AssetCache::new(2);

        let a = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();
        let b = cache.load::<GatedAsset>(&vfs, "/b.bin").unwrap();
        assert_eq!(cache.retained_count(), 2);
        // The consumer lets go of /a.bin; only the retention ring was keeping it
        // alive, and the next load is about to evict it.
        drop(a);

        let c = cache.load::<GatedAsset>(&vfs, "/c.bin").unwrap();
        drop(b);
        drop(c);

        let key_a = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "a.bin".to_string(),
        };
        let slots = cache.slots.read().unwrap_or_else(|p| p.into_inner());
        assert!(
            !slots.contains_key(&key_a),
            "dead weak slot entries must be pruned instead of accumulating"
        );
        assert_eq!(slots.len(), 2, "only live retained slots may remain");
    }

    #[test]
    fn f4b_topology_sync_drops_stale_retention() {
        let (mut vfs, _) = memory_vfs(&[("x.bin", b"old")]);
        let cache = AssetCache::new(8);

        let old = cache.load::<GatedAsset>(&vfs, "/x.bin").unwrap();
        assert_eq!(old.bytes, b"old".to_vec());

        // A higher-priority layer now shadows /x.bin.
        let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        mod_mem.set_file("x.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "mod",
            VfsPath::root(),
            100,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            mod_mem,
        ))
        .unwrap();
        cache.sync_topology(&vfs);

        let new = cache.load::<GatedAsset>(&vfs, "/x.bin").unwrap();
        assert_eq!(new.bytes, b"new".to_vec());
        drop(old);
        drop(new);

        let stale = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "x.bin".to_string(),
        };
        let retention = cache.retention.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            !retention.retained_handles.contains_key(&stale),
            "decoded bytes from a shadowed mount must not stay strongly retained"
        );
    }

    // ------------------------------------------------------------------ F5

    #[test]
    fn f5a_pin_racing_clear_returns_pin_invalidated() {
        let (vfs, _) = memory_vfs(&[("hud/bar.bin", b"bar")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/hud/bar.bin");

        let pinner = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.pin::<GatedAsset>(&v, "/hud/bar.bin"))
        };
        assert!(gate.wait_entered(GATE_WAIT), "pinner never entered decode");

        cache.clear();
        gate.release();
        let result = pinner.expect("pinner");
        match result {
            Err(crate::error::AssetError::PinInvalidated {
                reason: crate::error::PinInvalidation::RouteEvicted,
                ..
            }) => {}
            other => panic!("expected RouteEvicted PinInvalidated, got {other:?}"),
        }

        assert_eq!(
            cache.pinned_count(),
            0,
            "an invalidated pin must not leave pinned state"
        );
        assert!(!cache.is_pinned::<GatedAsset>("/hud/bar.bin"));
        assert!(cache.get::<GatedAsset>(&vfs, "/hud/bar.bin").is_none());
        assert_eq!(
            decode_calls("/hud/bar.bin"),
            1,
            "pin must not retry after invalidation"
        );
        remove_gate("/hud/bar.bin");
    }

    /// Deduplication invariant I7: concurrent identical loads decode once and
    /// share one Arc.
    #[test]
    fn f5c_concurrent_identical_loads_decode_once() {
        let (vfs, _) = memory_vfs(&[("shared.bin", b"shared")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/shared.bin");

        let first = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/shared.bin"))
        };
        assert!(
            gate.wait_entered(GATE_WAIT),
            "first loader never entered decode"
        );

        let second = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/shared.bin"))
        };
        // Give the second loader a moment to park on the in-flight barrier, then
        // release the decode. If it had not joined the barrier it would decode
        // again, so the decode count assertion below is the real check.
        std::thread::sleep(Duration::from_millis(50));
        gate.release();

        let a = first.expect("first loader").expect("first load");
        let b = second.expect("second loader").expect("second load");
        assert!(
            Arc::ptr_eq(&a, &b),
            "concurrent identical loads must share one Arc"
        );
        assert_eq!(
            decode_calls("/shared.bin"),
            1,
            "concurrent identical loads must decode exactly once"
        );
        remove_gate("/shared.bin");
    }

    // ------------------------------------------------------------------
    // Case-aware eviction, topology snapshots, and pin invalidation.
    // ------------------------------------------------------------------

    #[test]
    fn case_aware_evict_invalidates_all_case_variants() {
        let (vfs, _) =
            memory_vfs_with_case(&[("Images/BF.PNG", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        assert_eq!(old.bytes, b"old".to_vec());

        vfs.write("/IMAGES/BF.PNG", b"new").unwrap();
        cache.evict_path_case_aware(&vfs, "/Images/Bf.Png").unwrap();

        assert!(cache.get::<GatedAsset>(&vfs, "/images/bf.png").is_none());
        assert!(cache.get::<GatedAsset>(&vfs, "/IMAGES/BF.PNG").is_none());
        let fresh = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        assert_eq!(fresh.bytes, b"new".to_vec());
        let same = cache.load::<GatedAsset>(&vfs, "/IMAGES/BF.PNG").unwrap();
        assert!(Arc::ptr_eq(&fresh, &same));
    }

    #[test]
    fn case_aware_evict_preserves_sensitive_siblings() {
        let (vfs, _) = memory_vfs(&[("a/B.png", b"upper"), ("a/b.png", b"lower")]);
        let cache = AssetCache::new(8);
        let upper = cache.load::<GatedAsset>(&vfs, "/a/B.png").unwrap();
        let lower = cache.load::<GatedAsset>(&vfs, "/a/b.png").unwrap();

        vfs.write("/a/B.png", b"upper2").unwrap();
        cache.evict_path_case_aware(&vfs, "/a/B.png").unwrap();

        assert!(cache.get::<GatedAsset>(&vfs, "/a/B.png").is_none());
        let lower_again = cache.get::<GatedAsset>(&vfs, "/a/b.png").unwrap();
        assert_eq!(lower_again.bytes, b"lower".to_vec());
        assert!(Arc::ptr_eq(&lower, &lower_again));
        assert_eq!(
            cache.load::<GatedAsset>(&vfs, "/a/B.png").unwrap().bytes,
            b"upper2".to_vec()
        );
        drop(upper);
    }

    #[test]
    fn case_aware_evict_respects_overlapping_sensitive_mount() {
        let mut vfs = Vfs::new();
        let root = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        root.set_file("mods/foo/root.png", b"root").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            root,
        ))
        .unwrap();

        let nested = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        nested.set_file("asset.png", b"lower").unwrap();
        nested.set_file("Asset.png", b"upper").unwrap();
        vfs.mount(Mount::new(
            "nested",
            VfsPath::new("/mods/foo").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            nested,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let lower = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/asset.png")
            .unwrap();
        let upper = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/Asset.png")
            .unwrap();

        vfs.write("/mods/foo/asset.png", b"lower2").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/mods/foo/asset.png")
            .unwrap();

        assert!(cache
            .get::<GatedAsset>(&vfs, "/mods/foo/asset.png")
            .is_none());
        let upper_again = cache
            .get::<GatedAsset>(&vfs, "/mods/foo/Asset.png")
            .unwrap();
        assert!(Arc::ptr_eq(&upper, &upper_again));
        assert_eq!(upper_again.bytes, b"upper".to_vec());
        assert_eq!(
            cache
                .load::<GatedAsset>(&vfs, "/mods/foo/asset.png")
                .unwrap()
                .bytes,
            b"lower2".to_vec()
        );
        drop(lower);
    }

    #[test]
    fn case_aware_evict_removes_live_slot_without_route() {
        let (vfs, _) = memory_vfs_with_case(
            &[("LooseImages/BF.PNG", b"old")],
            CaseSensitivity::Insensitive,
        );
        let cache = AssetCache::new(8);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/looseimages/bf.png").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "LooseImages/BF.PNG".to_string(),
        };
        let _old = cache
            .load::<GatedAsset>(&vfs, "/looseimages/bf.png")
            .unwrap();

        // Simulate a retention/unpin window where the slot survives without a
        // route. The identity scan must still remove it.
        cache
            .router
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
        assert!(cache
            .slots
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&canonical));

        cache
            .evict_path_case_aware(&vfs, "/LooseImages/Bf.Png")
            .unwrap();
        assert!(
            !cache
                .slots
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&canonical),
            "a live slot with no route must still be invalidated by identity"
        );
    }

    #[test]
    fn remount_without_sync_different_casing_returns_new_content() {
        let (mut vfs, _) = memory_vfs_with_case(&[("a.bin", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();

        vfs.unmount("base").unwrap();
        let replacement = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        replacement.set_file("a.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            replacement,
        ))
        .unwrap();

        // No sync_topology and a spelling that had no route before the remount.
        let new = cache.load::<GatedAsset>(&vfs, "/A.bin").unwrap();
        assert_eq!(new.bytes, b"new".to_vec());
        assert_eq!(old.bytes, b"old".to_vec());
        assert!(!Arc::ptr_eq(&old, &new));
    }

    #[test]
    fn concurrent_loads_racing_remount_do_not_reuse_old_slot() {
        let (mut vfs, _) = memory_vfs_with_case(&[("a.bin", b"old")], CaseSensitivity::Insensitive);
        let cache = Arc::new(AssetCache::new(8));
        let old = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();

        vfs.unmount("base").unwrap();
        let replacement = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        replacement.set_file("a.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            replacement,
        ))
        .unwrap();
        let vfs = Arc::new(vfs);

        let start = Arc::new(std::sync::Barrier::new(9));
        let mut workers = Vec::new();
        for index in 0..8 {
            let (cache, vfs, start) = (cache.clone(), vfs.clone(), start.clone());
            let path = if index % 2 == 0 { "/a.bin" } else { "/A.bin" };
            workers.push(std::thread::spawn(move || {
                start.wait();
                cache
                    .load::<GatedAsset>(&vfs, path)
                    .map(|a| a.bytes.clone())
            }));
        }
        start.wait();

        for worker in workers {
            let bytes = worker.join().unwrap().expect("load");
            assert_eq!(bytes, b"new".to_vec());
        }
        assert_eq!(old.bytes, b"old".to_vec());
    }

    #[test]
    fn route_less_slot_of_removed_mount_is_evicted_by_exact_subtree() {
        let mut vfs = Vfs::new();
        let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        base.set_file("dir/root.txt", b"root").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            base,
        ))
        .unwrap();
        let removed = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        removed.set_file("x.bin", b"old").unwrap();
        vfs.mount(Mount::new(
            "removed",
            VfsPath::new("/dir/sub").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            removed,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/dir/sub/x.bin").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "removed".to_string(),
            canonical_relative_path: "x.bin".to_string(),
        };
        let _old = cache.load::<GatedAsset>(&vfs, "/dir/sub/x.bin").unwrap();
        cache
            .router
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
        assert!(cache
            .slots
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&canonical));

        vfs.unmount("removed").unwrap();
        cache.evict_path_case_aware(&vfs, "/dir").unwrap();
        assert!(
            !cache
                .slots
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&canonical),
            "the slot's remembered mount point must be enough after unmount"
        );
        assert_eq!(
            cache.retained_count(),
            0,
            "the retention entry's remembered mount point must also be enough"
        );
    }

    #[test]
    fn route_less_slot_of_removed_only_mount_is_evicted_without_target_mount() {
        let mut vfs = Vfs::new();
        let removed = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        removed.set_file("x.bin", b"old").unwrap();
        vfs.mount(Mount::new(
            "removed",
            VfsPath::new("/dir/sub").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            removed,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/dir/sub/x.bin").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "removed".to_string(),
            canonical_relative_path: "x.bin".to_string(),
        };
        let _old = cache.load::<GatedAsset>(&vfs, "/dir/sub/x.bin").unwrap();
        cache
            .router
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
        vfs.unmount("removed").unwrap();

        cache.evict_path_case_aware(&vfs, "/dir").unwrap();
        assert!(
            !cache
                .slots
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&canonical),
            "removed-mount slots must be resolvable without a current target mount"
        );
        assert_eq!(cache.retained_count(), 0);
    }

    #[test]
    fn unrelated_topology_change_reuses_same_instance_slot() {
        let (mut vfs, _) = memory_vfs(&[("a.bin", b"a")]);
        let cache = AssetCache::new(8);
        let first = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();

        let extra = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        extra.set_file("b.bin", b"b").unwrap();
        vfs.mount(Mount::new(
            "extra",
            VfsPath::new("/other").unwrap(),
            100,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            extra,
        ))
        .unwrap();

        let second = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();
        assert!(
            Arc::ptr_eq(&first, &second),
            "an unrelated topology change must not force re-decoding an unchanged mount instance"
        );
    }

    #[test]
    fn removed_mount_pin_is_evicted_by_exact_subtree() {
        let mut vfs = Vfs::new();
        let removed = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        removed.set_file("x.bin", b"old").unwrap();
        vfs.mount(Mount::new(
            "removed",
            VfsPath::new("/dir/sub").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            removed,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let pinned = cache.pin::<GatedAsset>(&vfs, "/dir/sub/x.bin").unwrap();
        assert!(cache.is_pinned::<GatedAsset>("/dir/sub/x.bin"));
        vfs.unmount("removed").unwrap();

        cache.evict_path_case_aware(&vfs, "/dir").unwrap();
        assert_eq!(cache.pinned_count(), 0);
        assert!(!cache.is_pinned::<GatedAsset>("/dir/sub/x.bin"));
        assert_eq!(pinned.bytes, b"old".to_vec());
    }

    #[test]
    fn remount_same_id_with_invalidation_returns_new_content() {
        let (mut vfs, _) = memory_vfs(&[("a.bin", b"old")]);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();
        assert_eq!(old.bytes, b"old".to_vec());
        let old_key = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "a.bin".to_string(),
        };

        vfs.unmount("base").unwrap();
        let replacement = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        replacement.set_file("a.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            replacement,
        ))
        .unwrap();
        cache.sync_topology(&vfs);

        assert!(
            !cache
                .slots
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&old_key),
            "topology invalidation must drop the old weak slot"
        );
        assert_eq!(old.bytes, b"old".to_vec(), "consumer Arc must remain valid");

        let new = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();
        assert_eq!(new.bytes, b"new".to_vec());
        assert!(!Arc::ptr_eq(&old, &new));
    }

    #[test]
    fn remount_without_sync_does_not_reuse_old_slot() {
        let (mut vfs, _) = memory_vfs(&[("a.bin", b"old")]);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();

        vfs.unmount("base").unwrap();
        let replacement = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        replacement.set_file("a.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            replacement,
        ))
        .unwrap();

        let new = cache.load::<GatedAsset>(&vfs, "/a.bin").unwrap();
        assert_eq!(new.bytes, b"new".to_vec());
        assert_eq!(old.bytes, b"old".to_vec());
        assert!(!Arc::ptr_eq(&old, &new));
    }

    #[test]
    fn pre_invalidation_decoder_cannot_install() {
        let (vfs, _) = memory_vfs(&[("h2/x.bin", b"old")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/h2/x.bin");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/h2/x.bin"))
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        cache.invalidate_topology();
        gate.release();
        let raced = loader.expect("loader").expect("load");
        assert_eq!(raced.bytes, b"old".to_vec());

        assert!(
            cache.get::<GatedAsset>(&vfs, "/h2/x.bin").is_none(),
            "a pre-invalidation decoder must not install after invalidate_topology returns"
        );
        let fresh = cache.load::<GatedAsset>(&vfs, "/h2/x.bin").unwrap();
        assert!(cache.get::<GatedAsset>(&vfs, "/h2/x.bin").is_some());
        assert_eq!(fresh.bytes, b"old".to_vec());
        remove_gate("/h2/x.bin");
    }

    #[test]
    fn write_and_evict_handles_nested_mount_and_case_insensitive_lower() {
        let mut vfs = Vfs::new();
        let lower = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        lower.set_file("Images/BF.PNG", b"old").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            lower,
        ))
        .unwrap();
        let upper = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        upper.set_file("Other.bin", b"other").unwrap();
        vfs.mount(Mount::new(
            "upper",
            VfsPath::new("/Images").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            upper,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        assert_eq!(old.bytes, b"old".to_vec());
        assert_eq!(
            vfs.write_target("/images/bf.png")
                .unwrap()
                .mount
                .unwrap()
                .id,
            "base",
            "the lower-case request must not route through the case-exact /Images mount"
        );

        cache
            .write_and_evict_path_case_aware(&vfs, "/images/bf.png", b"new")
            .unwrap();
        assert!(cache.get::<GatedAsset>(&vfs, "/images/bf.png").is_none());
        assert_eq!(
            cache
                .load::<GatedAsset>(&vfs, "/images/bf.png")
                .unwrap()
                .bytes,
            b"new".to_vec()
        );
        assert_eq!(old.bytes, b"old".to_vec());
    }

    #[test]
    fn write_and_evict_does_not_evict_when_write_fails() {
        let mut vfs = Vfs::new();
        let lower = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        lower.set_file("Images/BF.PNG", b"old").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            lower,
        ))
        .unwrap();
        let upper = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        vfs.mount(Mount::new(
            "upper",
            VfsPath::new("/Images").unwrap(),
            100,
            MountPolicy::ReadOnly,
            CaseSensitivity::Insensitive,
            upper,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/Images/BF.PNG").unwrap();
        let result = cache.write_and_evict_path_case_aware(&vfs, "/Images/BF.PNG", b"new");
        assert!(result.is_err(), "read-only top mount must fail the write");
        let still = cache.get::<GatedAsset>(&vfs, "/Images/BF.PNG").unwrap();
        assert!(Arc::ptr_eq(&old, &still));
    }

    #[test]
    fn case_aware_evict_removes_route_less_slot_on_other_mount() {
        let mut vfs = Vfs::new();
        let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        base.set_file("dir/root.txt", b"root").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            base,
        ))
        .unwrap();
        let nested = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        nested.set_file("x.bin", b"nested").unwrap();
        vfs.mount(Mount::new(
            "nested",
            VfsPath::new("/dir/sub").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            nested,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/dir/sub/x.bin").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "nested".to_string(),
            canonical_relative_path: "x.bin".to_string(),
        };
        let _old = cache.load::<GatedAsset>(&vfs, "/dir/sub/x.bin").unwrap();
        cache
            .router
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
        assert!(cache
            .slots
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&canonical));

        cache.evict_path_case_aware(&vfs, "/dir").unwrap();
        assert!(
            !cache
                .slots
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&canonical),
            "exact-subtree eviction must find a route-less slot on another mount"
        );
    }

    #[test]
    fn case_aware_evict_catches_shadow_on_higher_mount() {
        let mut vfs = Vfs::new();
        let root = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        root.set_file("mods/foo/loose.png", b"root-old").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            root,
        ))
        .unwrap();
        let nested = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        vfs.mount(Mount::new(
            "nested",
            VfsPath::new("/mods/foo").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            nested,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let old = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/loose.png")
            .unwrap();
        assert_eq!(old.bytes, b"root-old".to_vec());

        vfs.write("/mods/foo/LOOSE.PNG", b"shadow-new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/mods/foo/LOOSE.PNG")
            .unwrap();

        let fresh = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/loose.png")
            .unwrap();
        assert_eq!(fresh.bytes, b"shadow-new".to_vec());
    }

    #[test]
    fn case_aware_evict_does_not_case_fold_sensitive_shadow() {
        let mut vfs = Vfs::new();
        let root = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        root.set_file("mods/foo/loose.png", b"root-old").unwrap();
        vfs.mount(Mount::new(
            "base",
            VfsPath::root(),
            0,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            root,
        ))
        .unwrap();
        let nested = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        vfs.mount(Mount::new(
            "nested",
            VfsPath::new("/mods/foo").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Sensitive,
            nested,
        ))
        .unwrap();

        let cache = AssetCache::new(8);
        let old = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/loose.png")
            .unwrap();
        assert_eq!(old.bytes, b"root-old".to_vec());

        vfs.write("/mods/foo/LOOSE.PNG", b"shadow-new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/mods/foo/LOOSE.PNG")
            .unwrap();

        let still = cache
            .load::<GatedAsset>(&vfs, "/mods/foo/loose.png")
            .unwrap();
        assert_eq!(
            still.bytes,
            b"root-old".to_vec(),
            "a sensitive higher mount must not shadow a differently-cased lower path"
        );
    }

    #[test]
    fn case_aware_evict_is_component_aware() {
        let (vfs, _) = memory_vfs(&[("songs/a/x.bin", b"a"), ("songs/ab/y.bin", b"ab")]);
        let cache = AssetCache::new(8);
        let a = cache.load::<GatedAsset>(&vfs, "/songs/a/x.bin").unwrap();
        let ab = cache.load::<GatedAsset>(&vfs, "/songs/ab/y.bin").unwrap();

        cache.evict_path_case_aware(&vfs, "/songs/a").unwrap();

        assert!(cache.get::<GatedAsset>(&vfs, "/songs/a/x.bin").is_none());
        let ab_again = cache.get::<GatedAsset>(&vfs, "/songs/ab/y.bin").unwrap();
        assert!(Arc::ptr_eq(&ab, &ab_again));
        drop(a);
    }

    #[test]
    fn legacy_evict_path_is_exact_case() {
        let (vfs, _) =
            memory_vfs_with_case(&[("Images/BF.PNG", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();

        vfs.write("/IMAGES/BF.PNG", b"new").unwrap();
        cache.evict_path("/IMAGES/BF.PNG");

        let still = cache.get::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        assert!(
            Arc::ptr_eq(&old, &still),
            "legacy evict_path must remain exact-case and leave this route untouched"
        );
    }

    #[test]
    fn pre_eviction_case_variant_decoder_cannot_install() {
        let (vfs, _) = memory_vfs_with_case(
            &[("PreImages/BF.PNG", b"old")],
            CaseSensitivity::Insensitive,
        );
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/preimages/bf.png");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/preimages/bf.png"))
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        vfs.write("/PREIMAGES/BF.PNG", b"new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/PreImages/Bf.Png")
            .unwrap();

        assert!(cache.get::<GatedAsset>(&vfs, "/preimages/bf.png").is_none());
        gate.release();
        let raced = loader.expect("loader").expect("load returns its bytes");
        assert_eq!(raced.bytes, b"old".to_vec());

        assert!(
            cache.get::<GatedAsset>(&vfs, "/preimages/bf.png").is_none(),
            "a pre-eviction decoder must not install after eviction returns"
        );
        let fresh = cache.load::<GatedAsset>(&vfs, "/preimages/bf.png").unwrap();
        assert_eq!(fresh.bytes, b"new".to_vec());
        remove_gate("/preimages/bf.png");
    }

    #[test]
    fn post_eviction_same_casing_load_does_not_join_pre_eviction_decoder() {
        let (vfs, _) = memory_vfs_with_case(
            &[("JoinImages/BF.PNG", b"old")],
            CaseSensitivity::Insensitive,
        );
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/joinimages/bf.png");

        let first = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/joinimages/bf.png"))
        };
        assert!(
            gate.wait_entered(GATE_WAIT),
            "first loader never entered decode"
        );

        vfs.write("/JOINIMAGES/BF.PNG", b"new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/JoinImages/Bf.Png")
            .unwrap();

        let second = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/joinimages/bf.png"))
        };
        assert!(
            wait_until(GATE_WAIT, || decode_calls("/joinimages/bf.png") >= 2),
            "post-eviction loader must start its own decode, not join the old barrier"
        );
        gate.release();

        let old = first.expect("first loader").expect("first load");
        let new = second.expect("second loader").expect("second load");
        assert_eq!(old.bytes, b"old".to_vec());
        assert_eq!(new.bytes, b"new".to_vec());
        let cached = cache.get::<GatedAsset>(&vfs, "/joinimages/bf.png").unwrap();
        assert!(Arc::ptr_eq(&new, &cached));
        remove_gate("/joinimages/bf.png");
    }

    #[test]
    fn first_inserter_wins_cannot_persist_stale() {
        let (vfs, _) = memory_vfs_with_case(
            &[("FirstImages/BF.PNG", b"old")],
            CaseSensitivity::Insensitive,
        );
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let old_gate = install_gate("/firstimages/bf.png");
        let new_gate = install_gate("/FirstImages/Bf.Png");

        let old_loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/firstimages/bf.png"))
        };
        assert!(
            old_gate.wait_entered(GATE_WAIT),
            "old loader never entered decode"
        );

        vfs.write("/FirstImages/Bf.Png", b"new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/FirstImages/Bf.Png")
            .unwrap();

        let new_loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/FirstImages/Bf.Png"))
        };
        assert!(
            new_gate.wait_entered(GATE_WAIT),
            "new loader never entered decode"
        );

        old_gate.release();
        let old = old_loader.expect("old loader").expect("old load");
        assert_eq!(old.bytes, b"old".to_vec());
        new_gate.release();
        let new = new_loader.expect("new loader").expect("new load");
        assert_eq!(new.bytes, b"new".to_vec());

        let cached = cache
            .get::<GatedAsset>(&vfs, "/FirstImages/Bf.Png")
            .unwrap();
        assert!(
            Arc::ptr_eq(&new, &cached),
            "first-inserter-wins must not preserve a pre-eviction stale slot"
        );
        remove_gate("/firstimages/bf.png");
        remove_gate("/FirstImages/Bf.Png");
    }

    #[test]
    fn post_eviction_load_installs_fresh_content() {
        let (vfs, _) = memory_vfs_with_case(
            &[("PostImages/BF.PNG", b"old")],
            CaseSensitivity::Insensitive,
        );
        let cache = AssetCache::new(8);
        let old = cache
            .load::<GatedAsset>(&vfs, "/postimages/bf.png")
            .unwrap();
        assert_eq!(old.bytes, b"old".to_vec());

        vfs.write("/POSTIMAGES/BF.PNG", b"new").unwrap();
        cache
            .evict_path_case_aware(&vfs, "/PostImages/Bf.Png")
            .unwrap();

        let fresh = cache
            .load::<GatedAsset>(&vfs, "/postimages/bf.png")
            .unwrap();
        assert_eq!(fresh.bytes, b"new".to_vec());
        let cached = cache.get::<GatedAsset>(&vfs, "/postimages/bf.png").unwrap();
        assert!(Arc::ptr_eq(&fresh, &cached));
        drop(old);
    }

    #[test]
    fn pin_racing_case_aware_evict_returns_pin_invalidated() {
        let _pin_hook = PIN_HOOK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (vfs, _) = memory_vfs(&[("hud/evict.bin", b"bar")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let old = cache.load::<GatedAsset>(&vfs, "/hud/evict.bin").unwrap();
        let owner = Arc::as_ptr(&cache) as usize;
        let guard = HookGuard::new(&[Site::PinAfterLoadBeforeVerify]);

        test_hooks::arm(Site::PinAfterLoadBeforeVerify, owner);
        let pinner = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.pin::<GatedAsset>(&v, "/hud/evict.bin"))
        };
        assert!(test_hooks::wait_reached(
            Site::PinAfterLoadBeforeVerify,
            GATE_WAIT
        ));
        test_hooks::disarm(Site::PinAfterLoadBeforeVerify);

        cache.evict_path_case_aware(&vfs, "/hud/evict.bin").unwrap();
        test_hooks::release(Site::PinAfterLoadBeforeVerify);

        let result = pinner.expect("pinner");
        match result {
            Err(crate::error::AssetError::PinInvalidated {
                reason: crate::error::PinInvalidation::RouteEvicted,
                ..
            }) => {}
            other => panic!("expected RouteEvicted PinInvalidated, got {other:?}"),
        }
        assert_eq!(cache.pinned_count(), 0);
        drop(old);
        drop(guard);
    }

    #[test]
    fn journal_pruning_retains_intent_for_old_inflight() {
        let (vfs, _) = memory_vfs(&[("a.bin", b"a")]);
        let cache = AssetCache::new(8);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/dummy").unwrap(),
        };
        let barrier = Arc::new(InFlightBarrier::new());
        cache
            .in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                request.clone(),
                InFlightEntry {
                    barrier,
                    start_seq: 0,
                },
            );

        cache.evict_path_case_aware(&vfs, "/a.bin").unwrap();
        assert_eq!(
            cache
                .evictions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .len(),
            1,
            "the intent must survive while an older in-flight load exists"
        );

        cache
            .in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
        let in_flight = cache.in_flight.read().unwrap_or_else(|p| p.into_inner());
        cache.prune_eviction_intents(&in_flight);
        assert!(cache
            .evictions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty());
    }

    #[test]
    fn concurrent_evictions_assign_unique_increasing_sequences() {
        let (vfs, _) = memory_vfs(&[("a.bin", b"a"), ("b.bin", b"b")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/dummy").unwrap(),
        };
        let barrier = Arc::new(InFlightBarrier::new());
        cache
            .in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                request.clone(),
                InFlightEntry {
                    barrier,
                    start_seq: 0,
                },
            );

        let first = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.evict_path_case_aware(&v, "/a.bin"))
        };
        let second = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.evict_path_case_aware(&v, "/b.bin"))
        };
        assert!(first.expect("first eviction").is_ok());
        assert!(second.expect("second eviction").is_ok());

        let intents = cache.evictions.lock().unwrap_or_else(|p| p.into_inner());
        let mut seqs: Vec<u64> = intents.iter().map(|intent| intent.seq).collect();
        seqs.sort_unstable();
        assert_eq!(seqs, vec![1, 2]);
        drop(intents);
        cache
            .in_flight
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request);
    }

    #[test]
    fn intent_generation_mismatch_is_skipped() {
        let mount = MountStamp {
            id: "base".to_string(),
            instance_id: 1,
            mount_point: VfsPath::root(),
            priority: 0,
            case_sensitivity: CaseSensitivity::Insensitive,
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "Images/BF.PNG".to_string(),
        };
        let request = VfsPath::new("/images/bf.png").unwrap();
        let intent = EvictionIntent {
            seq: 1,
            topology_generation: 7,
            target: VfsPath::new("/IMAGES/BF.PNG").unwrap(),
            target_mount: Some(mount.clone()),
        };
        assert!(intent_affects(&intent, &request, &canonical, &mount, 7));
        assert!(!intent_affects(&intent, &request, &canonical, &mount, 8));
    }

    #[test]
    fn case_aware_evict_with_stale_snapshot_uses_fresh_mount() {
        let (mut vfs, _) =
            memory_vfs_with_case(&[("Images/BF.PNG", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        let stale = vfs.write_target("/IMAGES/BF.PNG").unwrap();

        let upper = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        upper.set_file("BF.PNG", b"new").unwrap();
        vfs.mount(Mount::new(
            "upper",
            VfsPath::new("/IMAGES").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            upper,
        ))
        .unwrap();

        let fresh = cache.load::<GatedAsset>(&vfs, "/IMAGES/bf.png").unwrap();
        assert_eq!(fresh.bytes, b"new".to_vec());
        cache
            .evict_path_with_snapshot(
                &vfs,
                VfsPath::new("/IMAGES/bf.png").unwrap(),
                stale,
                MAX_SNAPSHOT_ATTEMPTS,
            )
            .unwrap();

        assert!(
            cache.get::<GatedAsset>(&vfs, "/IMAGES/bf.png").is_none(),
            "the stale snapshot must be re-resolved to the new governing mount"
        );
        assert_eq!(
            cache
                .load::<GatedAsset>(&vfs, "/IMAGES/bf.png")
                .unwrap()
                .bytes,
            b"new".to_vec()
        );
        drop(old);
    }

    #[test]
    fn case_aware_evict_unstable_snapshot_leaves_state_unchanged() {
        let (mut vfs, _) =
            memory_vfs_with_case(&[("Images/BF.PNG", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let old = cache.load::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        let stale = vfs.write_target("/IMAGES/BF.PNG").unwrap();

        let upper = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
        upper.set_file("BF.PNG", b"new").unwrap();
        vfs.mount(Mount::new(
            "upper",
            VfsPath::new("/IMAGES").unwrap(),
            100,
            MountPolicy::ReadWrite,
            CaseSensitivity::Insensitive,
            upper,
        ))
        .unwrap();

        let fresh = cache.load::<GatedAsset>(&vfs, "/IMAGES/bf.png").unwrap();
        let cached_before = cache.cached_count();
        let routes_before = cache.router.read().unwrap_or_else(|p| p.into_inner()).len();

        let result =
            cache.evict_path_with_snapshot(&vfs, VfsPath::new("/IMAGES/bf.png").unwrap(), stale, 0);
        assert!(matches!(
            result,
            Err(crate::error::AssetError::TopologyChanged(_))
        ));
        assert_eq!(cache.cached_count(), cached_before);
        assert_eq!(
            cache.router.read().unwrap_or_else(|p| p.into_inner()).len(),
            routes_before
        );
        let still = cache.get::<GatedAsset>(&vfs, "/IMAGES/bf.png").unwrap();
        assert!(Arc::ptr_eq(&fresh, &still));
        drop(old);
    }

    #[test]
    fn pin_returns_pin_invalidated_on_topology_change() {
        let _pin_hook = PIN_HOOK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (vfs, _) = memory_vfs(&[("hud/bar.bin", b"bar")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let old = cache.load::<GatedAsset>(&vfs, "/hud/bar.bin").unwrap();
        let owner = Arc::as_ptr(&cache) as usize;
        let guard = HookGuard::new(&[Site::PinAfterLoadBeforeVerify]);

        test_hooks::arm(Site::PinAfterLoadBeforeVerify, owner);
        let pinner = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.pin::<GatedAsset>(&v, "/hud/bar.bin"))
        };
        assert!(
            test_hooks::wait_reached(Site::PinAfterLoadBeforeVerify, GATE_WAIT),
            "pinner never reached its hook"
        );
        test_hooks::disarm(Site::PinAfterLoadBeforeVerify);
        cache.invalidate_topology();
        assert!(
            cache
                .router
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .all(|route| route.generation == 0),
            "invalidate_topology must stale every route before the pin verifies"
        );
        test_hooks::release(Site::PinAfterLoadBeforeVerify);

        let result = pinner.expect("pinner");
        match result {
            Err(crate::error::AssetError::PinInvalidated {
                reason: crate::error::PinInvalidation::TopologyChanged,
                ..
            }) => {}
            other => panic!("expected TopologyChanged PinInvalidated, got {other:?}"),
        }
        assert_eq!(cache.pinned_count(), 0);
        drop(old);
        drop(guard);
    }

    #[test]
    fn pin_returns_pin_invalidated_when_handle_superseded() {
        let _pin_hook = PIN_HOOK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (vfs, _) = memory_vfs(&[("hud/bar.bin", b"old")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let old = cache.load::<GatedAsset>(&vfs, "/hud/bar.bin").unwrap();
        let owner = Arc::as_ptr(&cache) as usize;
        let guard = HookGuard::new(&[Site::PinAfterLoadBeforeVerify]);

        test_hooks::arm(Site::PinAfterLoadBeforeVerify, owner);
        let pinner = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.pin::<GatedAsset>(&v, "/hud/bar.bin"))
        };
        assert!(
            test_hooks::wait_reached(Site::PinAfterLoadBeforeVerify, GATE_WAIT),
            "pinner never reached its hook"
        );
        test_hooks::disarm(Site::PinAfterLoadBeforeVerify);

        let request = RequestKey {
            type_id: TypeId::of::<GatedAsset>(),
            path: VfsPath::new("/hud/bar.bin").unwrap(),
        };
        let canonical = CanonicalKey {
            type_id: TypeId::of::<GatedAsset>(),
            mount_id: "base".to_string(),
            canonical_relative_path: "hud/bar.bin".to_string(),
        };
        let mount = MountStamp::from_info(&vfs.get_mount_info("base").unwrap(), 1);
        let fresh: Arc<dyn Any + Send + Sync> = Arc::new(GatedAsset {
            bytes: b"new".to_vec(),
        });
        cache
            .router
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                request,
                RouteEntry {
                    canonical: canonical.clone(),
                    generation: vfs.topology_generation(),
                    mount: mount.clone(),
                },
            );
        cache
            .slots
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(canonical, SlotEntry::new(&fresh, &mount));

        test_hooks::release(Site::PinAfterLoadBeforeVerify);
        let result = pinner.expect("pinner");
        match result {
            Err(crate::error::AssetError::PinInvalidated {
                reason: crate::error::PinInvalidation::HandleSuperseded,
                ..
            }) => {}
            other => panic!("expected HandleSuperseded PinInvalidated, got {other:?}"),
        }
        assert_eq!(cache.pinned_count(), 0);
        drop(old);
        drop(guard);
    }

    #[test]
    fn pin_success_retains_exact_arc() {
        let (vfs, _) = memory_vfs(&[("hud/bar.bin", b"bar")]);
        let cache = AssetCache::new(8);
        let pinned = cache.pin::<GatedAsset>(&vfs, "/hud/bar.bin").unwrap();
        assert_eq!(cache.pinned_count(), 1);
        assert!(cache.is_pinned::<GatedAsset>("/hud/bar.bin"));
        let cached = cache.get::<GatedAsset>(&vfs, "/hud/bar.bin").unwrap();
        assert!(Arc::ptr_eq(&pinned, &cached));
        let pinned_any: Arc<dyn Any + Send + Sync> = pinned.clone();
        let handles = cache.pinned.read().unwrap_or_else(|p| p.into_inner());
        let handle = handles.values().next().expect("pinned handle");
        assert!(Arc::ptr_eq(&handle.handle, &pinned_any));
    }

    #[test]
    fn case_aware_evict_unpins_all_case_variants() {
        let (vfs, _) =
            memory_vfs_with_case(&[("Images/BF.PNG", b"old")], CaseSensitivity::Insensitive);
        let cache = AssetCache::new(8);
        let pinned = cache.pin::<GatedAsset>(&vfs, "/images/bf.png").unwrap();
        assert!(cache.is_pinned::<GatedAsset>("/images/bf.png"));

        vfs.write("/IMAGES/BF.PNG", b"new").unwrap();
        cache.evict_path_case_aware(&vfs, "/Images/Bf.Png").unwrap();

        assert_eq!(cache.pinned_count(), 0);
        assert!(!cache.is_pinned::<GatedAsset>("/images/bf.png"));
        assert!(cache.get::<GatedAsset>(&vfs, "/images/bf.png").is_none());
        assert_eq!(
            cache
                .load::<GatedAsset>(&vfs, "/IMAGES/BF.PNG")
                .unwrap()
                .bytes,
            b"new".to_vec()
        );
        drop(pinned);
    }

    #[test]
    fn repin_after_topology_supersedes_old_canonical() {
        let (mut vfs, _) = memory_vfs(&[("x.bin", b"old")]);
        let cache = AssetCache::new(8);
        let old = cache.pin::<GatedAsset>(&vfs, "/x.bin").unwrap();
        assert_eq!(cache.pinned_count(), 1);

        let upper = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
        upper.set_file("x.bin", b"new").unwrap();
        vfs.mount(Mount::new(
            "upper",
            VfsPath::root(),
            100,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            upper,
        ))
        .unwrap();
        cache.sync_topology(&vfs);

        let new = cache.pin::<GatedAsset>(&vfs, "/x.bin").unwrap();
        assert_eq!(new.bytes, b"new".to_vec());
        assert_eq!(
            cache.pinned_count(),
            1,
            "re-pinning must supersede the old canonical instead of leaking it"
        );
        let cached = cache.get::<GatedAsset>(&vfs, "/x.bin").unwrap();
        assert!(Arc::ptr_eq(&new, &cached));
        assert!(!Arc::ptr_eq(&old, &new));
    }

    #[test]
    fn case_aware_evict_does_not_invalidate_unrelated_inflight_load() {
        let (vfs, _) = memory_vfs(&[("a/x.bin", b"a"), ("b/y.bin", b"b")]);
        let cache = Arc::new(AssetCache::new(8));
        let vfs = Arc::new(vfs);
        let gate = install_gate("/b/y.bin");

        let loader = {
            let (c, v) = (cache.clone(), vfs.clone());
            spawn_task(move || c.load::<GatedAsset>(&v, "/b/y.bin"))
        };
        assert!(gate.wait_entered(GATE_WAIT), "loader never entered decode");

        cache.evict_path_case_aware(&vfs, "/a").unwrap();
        gate.release();
        let loaded = loader.expect("loader").expect("load");
        assert_eq!(loaded.bytes, b"b".to_vec());
        assert!(
            cache.get::<GatedAsset>(&vfs, "/b/y.bin").is_some(),
            "unrelated case-aware eviction must not discard this install"
        );
        remove_gate("/b/y.bin");
    }
}
