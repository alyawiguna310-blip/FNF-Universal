use std::sync::Arc;

use fu_vfs::{
    CaseSensitivity, MemoryMount, Mount, MountPolicy, Vfs, VfsBackend, VfsError, VfsPath,
};
use serde::{Deserialize, Serialize};

use crate::{
    index_files, index_vfs, index_vfs_directory, load_bytes, load_json, load_text, AssetError,
};

#[test]
fn test_index_vfs_missing_root_returns_error() {
    let vfs = Vfs::new();
    let err = index_vfs(&vfs, "/nonexistent").unwrap_err();
    match err {
        AssetError::Vfs(VfsError::NotFound(path)) => {
            assert_eq!(path.as_str(), "/nonexistent");
        }
        other => panic!("expected Vfs(NotFound), got {:?}", other),
    }
}

#[test]
fn test_index_vfs_empty_directory() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.create_dir("empty").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let files = index_vfs(&vfs, "/empty").unwrap();
    assert!(files.is_empty());
}

#[test]
fn test_index_vfs_recursive_and_deterministic_ordering() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("assets/zebra.txt", b"z").unwrap();
    mem.set_file("assets/b/file2.txt", b"2").unwrap();
    mem.set_file("assets/a/sub/file3.txt", b"3").unwrap();
    mem.set_file("assets/a/file1.txt", b"1").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let indexed = index_vfs(&vfs, "/assets").unwrap();
    let path_strings: Vec<&str> = indexed.iter().map(|p| p.as_str()).collect();

    // Verify recursion and strict lexicographical ordering
    assert_eq!(
        path_strings,
        vec![
            "/assets/a/file1.txt",
            "/assets/a/sub/file3.txt",
            "/assets/b/file2.txt",
            "/assets/zebra.txt",
        ]
    );

    // Verify alias function produces identical result
    let alias_indexed = index_vfs_directory(&vfs, "/assets").unwrap();
    assert_eq!(alias_indexed, indexed);
}

#[test]
fn test_index_vfs_nested_directories_and_root_query() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("root_file.txt", b"root").unwrap();
    mem.set_file("data/stages/stage.xml", b"<stage/>").unwrap();
    mem.set_file("data/characters/bf.xml", b"<bf/>").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    // Index entire VFS from root "/"
    let all_files = index_vfs(&vfs, "/").unwrap();
    let strings: Vec<&str> = all_files.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        strings,
        vec![
            "/data/characters/bf.xml",
            "/data/stages/stage.xml",
            "/root_file.txt",
        ]
    );

    // Index subfolder "/data"
    let data_files = index_vfs(&vfs, "/data").unwrap();
    let data_strings: Vec<&str> = data_files.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        data_strings,
        vec!["/data/characters/bf.xml", "/data/stages/stage.xml"]
    );
}

#[test]
fn test_index_vfs_overlay_mounts_deduplication() {
    let mut vfs = Vfs::new();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem.set_file("characters/bf.xml", b"base_bf").unwrap();
    base_mem
        .set_file("characters/dad.xml", b"base_dad")
        .unwrap();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem.set_file("characters/bf.xml", b"mod_bf").unwrap();
    mod_mem
        .set_file("characters/pico.xml", b"mod_pico")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    // Union index across mounts: bf.xml is deduplicated, dad.xml and pico.xml are retained
    let files = index_vfs(&vfs, "/characters").unwrap();
    let strings: Vec<&str> = files.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        strings,
        vec![
            "/characters/bf.xml",
            "/characters/dad.xml",
            "/characters/pico.xml",
        ]
    );
}

#[test]
fn test_file_loading_through_overlay_mount() {
    let mut vfs = Vfs::new();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem
        .set_file("songs/tutorial/Inst.ogg", b"base_audio")
        .unwrap();
    base_mem
        .set_file("songs/tutorial/Voices.ogg", b"base_voices")
        .unwrap();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem
        .set_file("songs/tutorial/Inst.ogg", b"mod_audio")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    // Mod overrides Inst.ogg
    let inst = load_bytes(&vfs, "/songs/tutorial/Inst.ogg").unwrap();
    assert_eq!(inst, b"mod_audio");

    // Falls back to base for Voices.ogg
    let voices = load_bytes(&vfs, "/songs/tutorial/Voices.ogg").unwrap();
    assert_eq!(voices, b"base_voices");

    // Missing file returns error
    let err = load_bytes(&vfs, "/songs/tutorial/Events.json").unwrap_err();
    assert!(matches!(err, AssetError::Vfs(VfsError::NotFound(_))));
}

#[test]
fn test_text_loading() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("data/stages/castle.xml", b"<stage name=\"castle\"/>")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let text = load_text(&vfs, "/data/stages/castle.xml").unwrap();
    assert_eq!(text, "<stage name=\"castle\"/>");

    let err = load_text(&vfs, "/nonexistent.txt").unwrap_err();
    assert!(matches!(err, AssetError::Vfs(VfsError::NotFound(_))));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct SampleChart {
    bpm: f64,
    speed: f32,
    song: String,
}

#[test]
fn test_json_loading_success_and_malformed() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file(
        "charts/tutorial.json",
        br#"{"bpm": 120.0, "speed": 2.5, "song": "Tutorial"}"#,
    )
    .unwrap();
    mem.set_file("charts/corrupt.json", b"{ not valid json }")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    // Success case
    let chart: SampleChart = load_json(&vfs, "/charts/tutorial.json").unwrap();
    assert_eq!(
        chart,
        SampleChart {
            bpm: 120.0,
            speed: 2.5,
            song: "Tutorial".into(),
        }
    );

    // Malformed JSON case: verify error carries virtual path
    let err = load_json::<SampleChart>(&vfs, "/charts/corrupt.json").unwrap_err();
    match err {
        AssetError::Json { path, source } => {
            assert_eq!(path.as_str(), "/charts/corrupt.json");
            assert!(!source.to_string().is_empty());
        }
        other => panic!("expected AssetError::Json, got {:?}", other),
    }

    // Missing JSON file
    let missing_err = load_json::<SampleChart>(&vfs, "/charts/missing.json").unwrap_err();
    assert!(matches!(
        missing_err,
        AssetError::Vfs(VfsError::NotFound(_))
    ));
}

#[test]
fn test_existing_index_files_compatibility() {
    // Tests that index_files still functions for host filesystem paths
    let paths = index_files(".").unwrap();
    assert!(!paths.is_empty());
}

use crate::{Asset, AssetCache};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, PartialEq, Eq)]
struct CountingAsset {
    data: String,
}

static DECODE_COUNT: AtomicUsize = AtomicUsize::new(0);

impl Asset for CountingAsset {
    type Error = std::convert::Infallible;
    fn decode(bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        DECODE_COUNT.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(CountingAsset {
            data: String::from_utf8_lossy(bytes).into_owned(),
        })
    }
}

#[derive(Debug)]
struct MockError(&'static str);

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MockError {}

#[derive(Debug)]
struct FailingAsset;

impl Asset for FailingAsset {
    type Error = MockError;
    fn decode(_bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        Err(MockError("corrupted asset format"))
    }
}

static PANIC_DECODE_CALLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static RELEASE_PANIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug)]
struct PanickingAsset;

impl Asset for PanickingAsset {
    type Error = std::convert::Infallible;
    fn decode(_bytes: &[u8], _path: &VfsPath) -> Result<Self, Self::Error> {
        PANIC_DECODE_CALLED.store(true, Ordering::SeqCst);
        while !RELEASE_PANIC.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("simulated decoder explosion");
    }
}

#[test]
fn test_cache_concurrent_load_deduplication() {
    DECODE_COUNT.store(0, Ordering::SeqCst);
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("shared/character.json", b"{\"name\":\"bf\"}")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let vfs = Arc::new(vfs);
    let cache = Arc::new(AssetCache::new(10));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let vfs_clone = vfs.clone();
        let cache_clone = cache.clone();
        handles.push(std::thread::spawn(move || {
            cache_clone
                .load::<CountingAsset>(&vfs_clone, "/shared/character.json")
                .unwrap()
        }));
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.len(), 8);
    // All 8 threads must receive the EXACT same Arc pointer
    for res in &results[1..] {
        assert!(Arc::ptr_eq(&results[0], res));
    }
    // Exactly 1 decode executed across all 8 concurrent requests
    assert_eq!(DECODE_COUNT.load(Ordering::SeqCst), 1);
}

#[test]
fn test_cache_concurrent_error_sharing() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("bad.dat", b"invalid_bytes").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let vfs = Arc::new(vfs);
    let cache = Arc::new(AssetCache::new(5));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let vfs_clone = vfs.clone();
        let cache_clone = cache.clone();
        handles.push(std::thread::spawn(move || {
            cache_clone.load::<FailingAsset>(&vfs_clone, "/bad.dat")
        }));
    }

    for h in handles {
        let res = h.join().unwrap();
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            AssetError::Shared(shared) => match &*shared {
                AssetError::Decode { path, reason } => {
                    assert_eq!(path.as_str(), "/bad.dat");
                    assert_eq!(reason, "corrupted asset format");
                }
                other => panic!("expected Decode error in Shared, got {:?}", other),
            },
            AssetError::Decode { path, reason } => {
                assert_eq!(path.as_str(), "/bad.dat");
                assert_eq!(reason, "corrupted asset format");
            }
            other => panic!("expected Shared or Decode error, got {:?}", other),
        }
    }
}

#[test]
fn test_cache_panic_safety_and_waiter_unblock() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("panic.dat", b"explode").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    PANIC_DECODE_CALLED.store(false, Ordering::SeqCst);
    RELEASE_PANIC.store(false, Ordering::SeqCst);

    let vfs = Arc::new(vfs);
    let cache = Arc::new(AssetCache::new(5));

    let vfs1 = vfs.clone();
    let cache1 = cache.clone();
    let panicking_handle = std::thread::spawn(move || {
        let _ = cache1.load::<PanickingAsset>(&vfs1, "/panic.dat");
    });

    // Wait until the panicking thread has entered decode and registered in_flight barrier
    while !PANIC_DECODE_CALLED.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let vfs2 = vfs.clone();
    let cache2 = cache.clone();
    let waiting_handle =
        std::thread::spawn(move || cache2.load::<PanickingAsset>(&vfs2, "/panic.dat"));

    // Allow waiting thread to query in_flight and wait on the barrier
    std::thread::sleep(std::time::Duration::from_millis(25));

    // Release loader thread to panic
    RELEASE_PANIC.store(true, Ordering::SeqCst);

    // Panicking thread terminates with panic
    let panic_res = panicking_handle.join();
    assert!(panic_res.is_err(), "thread should have panicked");

    // Waiting thread MUST NOT deadlock; it receives LoadAborted
    let wait_res = waiting_handle.join().unwrap();
    assert!(wait_res.is_err());
    let err = wait_res.unwrap_err();
    match err {
        AssetError::Shared(shared) => match &*shared {
            AssetError::LoadAborted(path) => {
                assert_eq!(path.as_str(), "/panic.dat");
            }
            other => panic!("expected LoadAborted, got {:?}", other),
        },
        AssetError::LoadAborted(path) => {
            assert_eq!(path.as_str(), "/panic.dat");
        }
        other => panic!(
            "expected Shared(LoadAborted) or LoadAborted, got {:?}",
            other
        ),
    }

    // Subsequent load can retry cleanly without being permanently stuck
    let retry_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cache.load::<PanickingAsset>(&vfs, "/panic.dat")
    }));
    assert!(
        retry_res.is_err(),
        "subsequent load retried and panicked anew"
    );
}

#[test]
fn test_cache_typed_identity_distinct_types_same_path() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("sample.txt", b"hello world").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let cache = AssetCache::new(10);

    let text_arc: Arc<String> = cache.load(&vfs, "/sample.txt").unwrap();
    let bytes_arc: Arc<Vec<u8>> = cache.load(&vfs, "/sample.txt").unwrap();

    assert_eq!(*text_arc, "hello world");
    assert_eq!(*bytes_arc, b"hello world");

    // Both types coexist in the cache under distinct TypeIds
    assert_eq!(cache.cached_count(), 2);

    let get_text: Arc<String> = cache.get(&vfs, "/sample.txt").unwrap();
    let get_bytes: Arc<Vec<u8>> = cache.get(&vfs, "/sample.txt").unwrap();
    assert!(Arc::ptr_eq(&text_arc, &get_text));
    assert!(Arc::ptr_eq(&bytes_arc, &get_bytes));
}

#[test]
fn test_cache_case_insensitive_casing_deduplication() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mem.set_file("Images/Boyfriend.PNG", b"bf_texture").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        mem,
    ))
    .unwrap();

    let cache = AssetCache::new(10);

    let first: Arc<String> = cache.load(&vfs, "/images/boyfriend.png").unwrap();
    let second: Arc<String> = cache.load(&vfs, "/IMAGES/BOYFRIEND.PNG").unwrap();

    // Both paths share the exact same underlying decoded Arc
    assert!(Arc::ptr_eq(&first, &second));
    // Cache slots hold only 1 canonical entry
    assert_eq!(cache.cached_count(), 1);
}

#[test]
fn test_cache_bounded_lru_retention() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("a.txt", b"a").unwrap();
    mem.set_file("b.txt", b"b").unwrap();
    mem.set_file("c.txt", b"c").unwrap();
    mem.set_file("d.txt", b"d").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    // Capacity of 2
    let cache = AssetCache::new(2);

    let a_handle: Arc<String> = cache.load(&vfs, "/a.txt").unwrap();
    let _b: Arc<String> = cache.load(&vfs, "/b.txt").unwrap();
    assert_eq!(cache.retained_count(), 2);
    assert_eq!(cache.cached_count(), 2);

    // Touch "a" again so "b" becomes the oldest
    let _ = cache.load::<String>(&vfs, "/a.txt").unwrap();

    // Load "c" -> triggers eviction of oldest ("b") from retention
    drop(_b);
    let _c: Arc<String> = cache.load(&vfs, "/c.txt").unwrap();
    assert_eq!(cache.retained_count(), 2);

    // "b" is now completely evicted from cache ownership because its consumer handle was dropped
    assert!(cache.get::<String>(&vfs, "/b.txt").is_none());

    // Load "d" -> triggers eviction of "a" from retention ring
    let _d: Arc<String> = cache.load(&vfs, "/d.txt").unwrap();
    assert_eq!(cache.retained_count(), 2);

    // "a" was evicted from retention ring, but caller still holds a_handle
    // Consumer handle remains 100% valid and usable
    assert_eq!(*a_handle, "a");
    // Since consumer still holds it, get() can still upgrade via weak slot
    assert!(cache.get::<String>(&vfs, "/a.txt").is_some());

    // Once consumer drops their handle, it is fully deallocated
    drop(a_handle);
    assert!(cache.get::<String>(&vfs, "/a.txt").is_none());

    // Prune dead entries
    cache.prune_expired();
    assert_eq!(cache.cached_count(), 2); // Only "c" and "d" remain
}

#[test]
fn test_cache_zero_retention_capacity() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("temp.txt", b"transient").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    // Capacity 0: unpinned assets are never retained by the cache ring
    let cache = AssetCache::new(0);

    let handle: Arc<String> = cache.load(&vfs, "/temp.txt").unwrap();
    assert_eq!(cache.retained_count(), 0);

    // Cache holds weak, caller holds sole strong reference
    assert_eq!(Arc::strong_count(&handle), 1);
    // While consumer holds handle, get() works
    assert!(cache.get::<String>(&vfs, "/temp.txt").is_some());

    // Once consumer drops handle, asset memory is freed immediately
    drop(handle);
    assert!(cache.get::<String>(&vfs, "/temp.txt").is_none());
    assert_eq!(cache.cached_count(), 0);
}

#[test]
fn test_cache_pin_unpin_lifecycle() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("ui/font.ttf", b"font_bytes").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let cache = AssetCache::new(2);

    assert!(!cache.is_pinned::<Vec<u8>>("/ui/font.ttf"));
    let pinned: Arc<Vec<u8>> = cache.pin(&vfs, "/ui/font.ttf").unwrap();
    assert!(cache.is_pinned::<Vec<u8>>("/ui/font.ttf"));
    assert_eq!(cache.pinned_count(), 1);
    assert_eq!(cache.retained_count(), 0); // Pinned assets bypass retention ring

    // Unpin returns asset to retention ring
    cache.unpin::<Vec<u8>>("/ui/font.ttf");
    assert!(!cache.is_pinned::<Vec<u8>>("/ui/font.ttf"));
    assert_eq!(cache.pinned_count(), 0);
    assert_eq!(cache.retained_count(), 1);
    drop(pinned);
}

#[test]
fn test_cache_eviction_and_path_eviction() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("music/inst.ogg", b"inst").unwrap();
    mem.set_file("music/voices.ogg", b"voices").unwrap();
    mem.set_file("ui/cursor.png", b"cursor").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let cache = AssetCache::new(10);
    let _inst: Arc<Vec<u8>> = cache.load(&vfs, "/music/inst.ogg").unwrap();
    let _voices: Arc<Vec<u8>> = cache.load(&vfs, "/music/voices.ogg").unwrap();
    let _cursor: Arc<Vec<u8>> = cache.load(&vfs, "/ui/cursor.png").unwrap();
    assert_eq!(cache.cached_count(), 3);

    // Evict single asset
    cache.evict::<Vec<u8>>("/ui/cursor.png");
    assert_eq!(cache.cached_count(), 2);
    assert!(cache.get::<Vec<u8>>(&vfs, "/ui/cursor.png").is_none());

    // Evict path prefix "/music"
    cache.evict_path("/music");
    assert_eq!(cache.cached_count(), 0);
    assert!(cache.get::<Vec<u8>>(&vfs, "/music/inst.ogg").is_none());
}

#[test]
fn test_cache_mount_topology_invalidation() {
    let mut vfs = Vfs::new();
    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem.set_file("song.json", b"base_chart").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    let cache = AssetCache::new(10);

    let first: Arc<String> = cache.load(&vfs, "/song.json").unwrap();
    assert_eq!(*first, "base_chart");

    // Mount mod with higher priority (modifies topology)
    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem.set_file("song.json", b"mod_chart").unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    // Cache syncs topology with VFS
    cache.sync_topology(&vfs);

    // Fast in-memory get returns None because route is invalidated
    assert!(cache.get::<String>(&vfs, "/song.json").is_none());

    // Subsequent load re-resolves and loads mod asset
    let second: Arc<String> = cache.load(&vfs, "/song.json").unwrap();
    assert_eq!(*second, "mod_chart");
    assert!(!Arc::ptr_eq(&first, &second));
}

#[test]
fn test_topology_invalidation_preserves_pinning_and_eviction() {
    let mut vfs = Vfs::new();
    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem.set_file("fonts/vcr.ttf", b"vcr_font").unwrap();
    base_mem.set_file("old_data.txt", b"old").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    let cache = AssetCache::new(10);

    // 1. Pin an asset before topology changes
    let font = cache.pin::<Vec<u8>>(&vfs, "/fonts/vcr.ttf").unwrap();
    let _old = cache.load::<String>(&vfs, "/old_data.txt").unwrap();
    assert!(cache.is_pinned::<Vec<u8>>("/fonts/vcr.ttf"));
    assert_eq!(cache.pinned_count(), 1);

    // 2. Change VFS topology (mount a new higher-priority layer)
    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem.set_file("mod.txt", b"mod").unwrap();
    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    // 3. Synchronize topology
    cache.sync_topology(&vfs);

    // 4. Verify is_pinned() remains correct!
    assert!(
        cache.is_pinned::<Vec<u8>>("/fonts/vcr.ttf"),
        "pinned state must survive topology synchronization"
    );
    assert_eq!(cache.pinned_count(), 1);

    // 5. Unpin it and verify pinned count decreases
    cache.unpin::<Vec<u8>>("/fonts/vcr.ttf");
    assert!(!cache.is_pinned::<Vec<u8>>("/fonts/vcr.ttf"));
    assert_eq!(cache.pinned_count(), 0);

    // 6. Verify eviction still works for assets loaded before topology changes
    assert!(cache.get::<String>(&vfs, "/old_data.txt").is_none()); // route was invalidated
                                                                   // Re-load old_data under new topology
    let _ = cache.load::<String>(&vfs, "/old_data.txt").unwrap();
    assert_eq!(
        cache.cached_count(),
        1,
        "topology invalidation drops the font's weak slot; only old_data has a slot"
    );
    assert_eq!(
        cache.retained_count(),
        2,
        "font and old_data are both retained after the unpin"
    );
    // Explicit eviction removes old_data cleanly. The font remains retained but
    // has no weak slot after topology invalidation until it is loaded again.
    cache.evict::<String>("/old_data.txt");
    assert_eq!(cache.cached_count(), 0);
    assert_eq!(cache.retained_count(), 1, "only the font remains retained");
    drop(font);
}

#[test]
fn test_concurrent_canonical_identity_collision_single_decode() {
    DECODE_COUNT.store(0, Ordering::SeqCst);
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mem.set_file("Images/Icons/Icon-Health.png", b"icon_data")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        mem,
    ))
    .unwrap();

    let vfs = Arc::new(vfs);
    let cache = Arc::new(AssetCache::new(10));
    let start_barrier = Arc::new(std::sync::Barrier::new(3));

    let b1 = start_barrier.clone();
    let v1 = vfs.clone();
    let c1 = cache.clone();
    let handle1 = std::thread::spawn(move || {
        b1.wait();
        c1.load::<CountingAsset>(&v1, "/images/icons/icon-health.png")
            .unwrap()
    });

    let b2 = start_barrier.clone();
    let v2 = vfs.clone();
    let c2 = cache.clone();
    let handle2 = std::thread::spawn(move || {
        b2.wait();
        c2.load::<CountingAsset>(&v2, "/IMAGES/ICONS/ICON-HEALTH.PNG")
            .unwrap()
    });

    // Release both threads simultaneously
    start_barrier.wait();

    let res1 = handle1.join().unwrap();
    let res2 = handle2.join().unwrap();

    // Must share identical Arc pointer
    assert!(
        Arc::ptr_eq(&res1, &res2),
        "concurrent requests with different casing must return the same Arc instance"
    );

    // Must execute decode EXACTLY ONCE
    assert_eq!(
        DECODE_COUNT.load(Ordering::SeqCst),
        1,
        "concurrent requests with differing casings must not duplicate decode work"
    );
}

#[test]
fn test_pin_operation_concurrency_robustness() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("hud/healthBar.png", b"health_bar_bytes")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let vfs = Arc::new(vfs);
    let cache = Arc::new(AssetCache::new(5));

    // Concurrently pin and sync_topology / evict
    let mut handles = Vec::new();
    for _ in 0..4 {
        let v = vfs.clone();
        let c = cache.clone();
        handles.push(std::thread::spawn(move || {
            c.pin::<Vec<u8>>(&v, "/hud/healthBar.png")
        }));
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for res in results {
        assert!(res.is_ok(), "pin must succeed reliably under concurrency");
    }

    assert!(cache.is_pinned::<Vec<u8>>("/hud/healthBar.png"));
    assert_eq!(cache.pinned_count(), 1);
}
