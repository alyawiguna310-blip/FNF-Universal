use std::sync::Arc;

use crate::backend::VfsBackend;
use crate::error::VfsError;
use crate::memory::MemoryMount;
use crate::mount::Mount;
use crate::path::VfsPath;
use crate::types::{CaseSensitivity, MountPolicy, VfsFileType};
use crate::vfs::Vfs;

#[test]
fn write_target_reports_mount_instance() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("x.bin", b"x").unwrap();
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let target = vfs.write_target("/x.bin").unwrap();
    assert_eq!(target.mount_instance, vfs.mount_instance_id("base"));
    assert!(target.mount_instance.is_some());
}

#[test]
fn mount_instance_changes_on_remount_and_survives_enable_disable() {
    let mut vfs = Vfs::new();
    let first = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        first,
    ))
    .unwrap();
    let first_instance = vfs.mount_instance_id("base").unwrap();

    vfs.set_enabled("base", false).unwrap();
    assert_eq!(vfs.mount_instance_id("base"), Some(first_instance));
    vfs.set_enabled("base", true).unwrap();
    assert_eq!(vfs.mount_instance_id("base"), Some(first_instance));

    vfs.unmount("base").unwrap();
    assert_eq!(vfs.mount_instance_id("base"), None);

    let second = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        second,
    ))
    .unwrap();
    let second_instance = vfs.mount_instance_id("base").unwrap();
    assert_ne!(first_instance, second_instance);
}

#[test]
fn write_target_reports_current_generation_and_mount() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mem.set_file("x.bin", b"x").unwrap();
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Insensitive,
        mem,
    ))
    .unwrap();

    let target = vfs.write_target("/x.bin").unwrap();
    assert_eq!(target.topology_generation, vfs.topology_generation());
    let mount = target.mount.expect("governing mount");
    assert_eq!(mount.id, "base");
    assert_eq!(mount.case_sensitivity, CaseSensitivity::Insensitive);
}

#[test]
fn write_target_tracks_mount_enable_disable_and_unmount() {
    let mut vfs = Vfs::new();
    let root = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        root,
    ))
    .unwrap();
    let generation_after_base = vfs.topology_generation();

    let nested = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    vfs.mount(Mount::new(
        "nested",
        VfsPath::new("/nested").unwrap(),
        100,
        MountPolicy::ReadWrite,
        CaseSensitivity::Insensitive,
        nested,
    ))
    .unwrap();
    let after_mount = vfs.write_target("/nested/x.bin").unwrap();
    assert_eq!(after_mount.mount.unwrap().id, "nested");
    assert!(after_mount.topology_generation > generation_after_base);

    vfs.set_enabled("nested", false).unwrap();
    let disabled = vfs.write_target("/nested/x.bin").unwrap();
    assert_eq!(disabled.mount.unwrap().id, "base");
    assert!(disabled.topology_generation > after_mount.topology_generation);

    vfs.set_enabled("nested", true).unwrap();
    let enabled = vfs.write_target("/nested/x.bin").unwrap();
    assert_eq!(enabled.mount.unwrap().id, "nested");

    vfs.unmount("nested").unwrap();
    let unmounted = vfs.write_target("/nested/x.bin").unwrap();
    assert_eq!(unmounted.mount.unwrap().id, "base");
    assert!(unmounted.topology_generation > enabled.topology_generation);
}

#[test]
fn write_target_none_mount_still_reports_generation() {
    let vfs = Vfs::new();
    let target = vfs.write_target("/anything").unwrap();
    assert!(target.mount.is_none());
    assert_eq!(target.topology_generation, vfs.topology_generation());
    assert!(vfs.write_target("C:/invalid").is_err());
}

#[test]
fn write_target_is_top_candidate_even_when_lower_mount_has_file() {
    let mut vfs = Vfs::new();
    let root = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root.set_file("nested/x.bin", b"root").unwrap();
    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        root,
    ))
    .unwrap();
    let nested = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    vfs.mount(Mount::new(
        "nested",
        VfsPath::new("/nested").unwrap(),
        100,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        nested,
    ))
    .unwrap();

    // The read resolver falls through to root, but a write would create the
    // shadow on the nested mount.
    assert_eq!(vfs.resolve_mount("/nested/x.bin").unwrap().id, "base");
    assert_eq!(
        vfs.write_target("/nested/x.bin").unwrap().mount.unwrap().id,
        "nested"
    );
}

#[test]
fn test_virtual_path_normalization() {
    assert_eq!(VfsPath::new("").unwrap().as_str(), "/");
    assert_eq!(VfsPath::new("/").unwrap().as_str(), "/");
    assert_eq!(VfsPath::new("///").unwrap().as_str(), "/");
    assert_eq!(
        VfsPath::new("images/bg.png").unwrap().as_str(),
        "/images/bg.png"
    );
    assert_eq!(
        VfsPath::new("/assets//data///stages/./castle.xml")
            .unwrap()
            .as_str(),
        "/assets/data/stages/castle.xml"
    );
    assert_eq!(
        VfsPath::new(r"assets\images\characters\bf.png")
            .unwrap()
            .as_str(),
        "/assets/images/characters/bf.png"
    );
}

#[test]
fn test_invalid_path_rejection() {
    assert!(matches!(
        VfsPath::new("C:/Windows/System32"),
        Err(VfsError::InvalidPath(_))
    ));
    assert!(matches!(
        VfsPath::new("foo\0bar"),
        Err(VfsError::InvalidPath(_))
    ));
}

#[test]
fn test_path_traversal_rejection() {
    assert!(matches!(
        VfsPath::new("../secret"),
        Err(VfsError::PathTraversal(_))
    ));
    assert!(matches!(
        VfsPath::new("/assets/data/../../etc/passwd"),
        Err(VfsError::PathTraversal(_))
    ));
    assert!(matches!(
        VfsPath::new(r"assets\..\..\secrets"),
        Err(VfsError::PathTraversal(_))
    ));
}

#[test]
fn test_mount_registration() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    let mount = Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    );
    assert!(vfs.mount(mount).is_ok());

    let mounts = vfs.list_mounts();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].id, "base");
    assert_eq!(mounts[0].priority, 0);
    assert!(vfs.get_mount_info("base").is_some());
}

#[test]
fn test_duplicate_mount_identifiers() {
    let mut vfs = Vfs::new();
    let mem1 = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    let mem2 = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem1,
    ))
    .unwrap();

    let err = vfs
        .mount(Mount::new(
            "base",
            VfsPath::root(),
            10,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            mem2,
        ))
        .unwrap_err();

    assert_eq!(err, VfsError::DuplicateMount("base".into()));
}

#[test]
fn test_mount_priority_and_mod_asset_overrides() {
    let mut vfs = Vfs::new();

    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("images/bg.png", b"base_background").unwrap();
    base.set_file("images/ui.png", b"base_ui").unwrap();

    let mod_a = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_a
        .set_file("images/bg.png", b"mod_a_background")
        .unwrap();

    let mod_b = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_b
        .set_file("images/bg.png", b"mod_b_background")
        .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "mod_a",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_a,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "mod_b",
        VfsPath::root(),
        50,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_b,
    ))
    .unwrap();

    // Mod B has highest priority (50 > 10 > 0)
    assert_eq!(
        vfs.read_to_string("/images/bg.png").unwrap(),
        "mod_b_background"
    );
    assert_eq!(vfs.resolve_mount("/images/bg.png").unwrap().id, "mod_b");
}

#[test]
fn test_base_game_fallback() {
    let mut vfs = Vfs::new();

    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("images/bg.png", b"base_bg").unwrap();
    base.set_file("data/stages/stage.xml", b"<stage/>").unwrap();

    let mod_a = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_a.set_file("images/bg.png", b"mod_bg").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "mod_a",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_a,
    ))
    .unwrap();

    // Mod overrides bg.png
    assert_eq!(vfs.read_to_string("/images/bg.png").unwrap(), "mod_bg");
    // Mod does not have stage.xml, so it falls back to base
    assert_eq!(
        vfs.read_to_string("/data/stages/stage.xml").unwrap(),
        "<stage/>"
    );
    assert_eq!(
        vfs.resolve_mount("/data/stages/stage.xml").unwrap().id,
        "base"
    );
}

#[test]
fn test_missing_asset_behavior() {
    let mut vfs = Vfs::new();
    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("images/bg.png", b"base_bg").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();

    assert!(!vfs.exists("/images/missing.png"));
    let err = vfs.read("/images/missing.png").unwrap_err();
    assert_eq!(
        err,
        VfsError::NotFound(VfsPath::new("/images/missing.png").unwrap())
    );
}

#[test]
fn test_readonly_mount_restrictions() {
    let mut vfs = Vfs::new();
    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();

    let err = vfs.write("/images/new.png", b"malicious").unwrap_err();
    assert!(matches!(err, VfsError::ReadOnlyMount { .. }));
}

#[test]
fn test_writable_user_data_mount() {
    let mut vfs = Vfs::new();

    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("config/default.json", b"{\"volume\":1.0}")
        .unwrap();

    let user = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "user_saves",
        VfsPath::new("/saves").unwrap(),
        10,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        user,
    ))
    .unwrap();

    // Write to writable mount
    vfs.write("/saves/scores.json", b"{\"score\":9999}")
        .unwrap();
    assert_eq!(
        vfs.read_to_string("/saves/scores.json").unwrap(),
        "{\"score\":9999}"
    );

    // Cannot write to read-only root mount
    assert!(matches!(
        vfs.write("/config/default.json", b"overwrite"),
        Err(VfsError::ReadOnlyMount { .. })
    ));
}

#[test]
fn test_case_sensitivity_policies() {
    let mut vfs_sensitive = Vfs::new();
    let mem_sensitive = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem_sensitive
        .set_file("images/Neu.png", b"case_sensitive_art")
        .unwrap();
    vfs_sensitive
        .mount(Mount::new(
            "test",
            VfsPath::root(),
            0,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            mem_sensitive,
        ))
        .unwrap();

    assert!(vfs_sensitive.exists("/images/Neu.png"));
    assert!(!vfs_sensitive.exists("/images/neu.png"));
    assert!(!vfs_sensitive.exists("/images/NEU.PNG"));

    let mut vfs_insensitive = Vfs::new();
    let mem_insensitive = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mem_insensitive
        .set_file("images/Neu.png", b"case_insensitive_art")
        .unwrap();
    vfs_insensitive
        .mount(Mount::new(
            "test",
            VfsPath::root(),
            0,
            MountPolicy::ReadOnly,
            CaseSensitivity::Insensitive,
            mem_insensitive,
        ))
        .unwrap();

    assert!(vfs_insensitive.exists("/images/Neu.png"));
    assert!(vfs_insensitive.exists("/images/neu.png"));
    assert!(vfs_insensitive.exists("/images/NEU.PNG"));
    assert_eq!(
        vfs_insensitive.read_to_string("/images/neu.png").unwrap(),
        "case_insensitive_art"
    );
}

#[test]
fn test_mount_enable_disable_behavior() {
    let mut vfs = Vfs::new();

    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("song.ogg", b"base_audio").unwrap();

    let mod_mount = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mount.set_file("song.ogg", b"mod_audio").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mount,
    ))
    .unwrap();

    // Mod enabled: reads mod audio
    assert_eq!(vfs.read_to_string("/song.ogg").unwrap(), "mod_audio");

    // Disable mod
    vfs.set_enabled("mod", false).unwrap();
    assert_eq!(vfs.read_to_string("/song.ogg").unwrap(), "base_audio");

    // Re-enable mod
    vfs.set_enabled("mod", true).unwrap();
    assert_eq!(vfs.read_to_string("/song.ogg").unwrap(), "mod_audio");
}

#[test]
fn test_deterministic_resolution_order() {
    let mut vfs = Vfs::new();

    let root_mount = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mount
        .set_file("mods/demo/meta.json", b"root_meta")
        .unwrap();

    let specific_mount = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    specific_mount
        .set_file("meta.json", b"specific_meta")
        .unwrap();

    // specific_mount has LOWER priority (-10), but more specific mount point ("/mods/demo")
    vfs.mount(Mount::new(
        "root",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mount,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "specific",
        VfsPath::new("/mods/demo").unwrap(),
        -10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        specific_mount,
    ))
    .unwrap();

    // Prefix specificity wins over global priority
    assert_eq!(
        vfs.read_to_string("/mods/demo/meta.json").unwrap(),
        "specific_meta"
    );
    assert_eq!(
        vfs.resolve_mount("/mods/demo/meta.json").unwrap().id,
        "specific"
    );
}

#[test]
fn test_platform_independent_path_behavior() {
    let mut vfs = Vfs::new();
    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("data/characters/bf.xml", b"<character/>")
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

    // Querying with Windows-style backslashes works identically to standard forward slashes
    assert!(vfs.exists(r"data\characters\bf.xml"));
    assert_eq!(
        vfs.read_to_string(r"data\characters\bf.xml").unwrap(),
        "<character/>"
    );
}

#[test]
fn test_union_directory_listing() {
    let mut vfs = Vfs::new();

    let base = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base.set_file("characters/bf.xml", b"<bf/>").unwrap();
    base.set_file("characters/dad.xml", b"<dad/>").unwrap();

    let mod_mount = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mount
        .set_file("characters/bf.xml", b"<mod_bf/>")
        .unwrap();
    mod_mount.set_file("characters/neu.xml", b"<neu/>").unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mount,
    ))
    .unwrap();

    let entries = vfs.read_dir("/characters").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

    // Union listing deduplicates "bf.xml" and includes "dad.xml" and "neu.xml"
    assert_eq!(names, vec!["bf.xml", "dad.xml", "neu.xml"]);
    assert!(entries.iter().all(|e| e.file_type == VfsFileType::File));
}

#[test]
fn test_physical_mount_isolation_and_safety() {
    let temp_dir = std::env::temp_dir().join(format!("fu_vfs_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("sub")).unwrap();

    std::fs::write(temp_dir.join("test.txt"), b"disk_content").unwrap();

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "temp_mount",
        "/temp",
        &temp_dir,
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
    )
    .unwrap();

    assert_eq!(
        vfs.read_to_string("/temp/test.txt").unwrap(),
        "disk_content"
    );

    // Write to physical mount
    vfs.write("/temp/new_file.txt", b"new_data").unwrap();
    assert_eq!(
        vfs.read_to_string("/temp/new_file.txt").unwrap(),
        "new_data"
    );
    assert_eq!(
        std::fs::read(temp_dir.join("new_file.txt")).unwrap(),
        b"new_data"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(temp_dir);
}

fn create_test_link(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link.as_os_str())
            .arg(target.as_os_str())
            .status();
        match status {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

#[test]
fn test_symlink_escape_existing_and_nested_nonexistent() {
    let base_temp =
        std::env::temp_dir().join(format!("fu_vfs_symlink_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base_temp);

    let mount_dir = base_temp.join("mount_root");
    let outside_dir = base_temp.join("outside_root");
    std::fs::create_dir_all(&mount_dir).unwrap();
    std::fs::create_dir_all(&outside_dir).unwrap();

    std::fs::write(outside_dir.join("secret.txt"), b"confidential").unwrap();

    let link_path = mount_dir.join("link_to_outside");
    if !create_test_link(&outside_dir, &link_path) {
        // Skip test if environment forbids symlink/junction creation
        let _ = std::fs::remove_dir_all(&base_temp);
        return;
    }

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "sandboxed",
        "/sandbox",
        &mount_dir,
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
    )
    .unwrap();

    // 1. Existing file accessed via outside symlink is rejected
    let read_res = vfs.read("/sandbox/link_to_outside/secret.txt");
    assert!(
        matches!(read_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on symlink escape, got {:?}",
        read_res
    );

    // 2. Writing to a nonexistent nested path under outside symlink is rejected via nearest ancestor validation
    let write_res = vfs.write("/sandbox/link_to_outside/sub/hack.txt", b"exploit");
    assert!(
        matches!(write_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on write escape, got {:?}",
        write_res
    );

    // 3. Creating a nonexistent nested directory under outside symlink is rejected
    let mkdir_res = vfs.create_dir("/sandbox/link_to_outside/new_folder");
    assert!(
        matches!(mkdir_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on mkdir escape, got {:?}",
        mkdir_res
    );

    // 4. Outside directory and files remain completely untouched (never deleted or modified)
    assert!(outside_dir.join("secret.txt").exists());
    assert_eq!(
        std::fs::read(outside_dir.join("secret.txt")).unwrap(),
        b"confidential"
    );
    assert!(!outside_dir.join("sub").exists());
    assert!(!outside_dir.join("new_folder").exists());

    // Cleanup
    let _ = std::fs::remove_dir_all(&base_temp);
}

#[test]
fn test_outside_target_remains_untouched_on_escape() {
    let base_temp =
        std::env::temp_dir().join(format!("fu_vfs_untouched_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base_temp);

    let mount_dir = base_temp.join("mount");
    let outside_dir = base_temp.join("outside");
    let outside_subdir = outside_dir.join("existing_subdir");
    std::fs::create_dir_all(&mount_dir).unwrap();
    std::fs::create_dir_all(&outside_subdir).unwrap();

    let outside_file = outside_dir.join("critical.dat");
    std::fs::write(&outside_file, b"untouchable host data").unwrap();

    let link_path = mount_dir.join("escape_link");
    if !create_test_link(&outside_dir, &link_path) {
        let _ = std::fs::remove_dir_all(&base_temp);
        return;
    }

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "rw_sandbox",
        "/",
        &mount_dir,
        0,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
    )
    .unwrap();

    // 1. Attempt write to an existing outside file via the link
    let write_res = vfs.write("/escape_link/critical.dat", b"overwritten payload");
    assert!(
        matches!(write_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on write escape attempt"
    );
    // Crucial check: outside file MUST still exist and MUST NOT have been overwritten or deleted
    assert!(outside_file.exists(), "outside file was deleted!");
    assert_eq!(
        std::fs::read(&outside_file).unwrap(),
        b"untouchable host data",
        "outside file content was modified!"
    );

    // 2. Attempt create_dir on an existing outside directory via the link
    let mkdir_res = vfs.create_dir("/escape_link/existing_subdir");
    assert!(
        matches!(mkdir_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on create_dir escape attempt"
    );
    // Crucial check: outside directory MUST still exist and MUST NOT have been removed by any cleanup
    assert!(
        outside_subdir.exists(),
        "outside existing directory was deleted by cleanup!"
    );

    // 3. Attempt create_dir on a nonexistent nested outside directory
    let mkdir_nested_res = vfs.create_dir("/escape_link/nonexistent_sub/new_leaf");
    assert!(
        matches!(mkdir_nested_res, Err(VfsError::PathTraversal(_))),
        "expected PathTraversal on nested create_dir escape attempt"
    );
    assert!(
        !outside_dir.join("nonexistent_sub").exists(),
        "nonexistent outside directory should not have been created"
    );

    let _ = std::fs::remove_dir_all(&base_temp);
}

#[test]
fn test_write_isolation_overlapping_readonly_and_writable() {
    let mut vfs = Vfs::new();

    // Higher-priority read-only mod at "/" (priority 100)
    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem.set_file("config.json", b"mod_config").unwrap();

    // Lower-priority writable user storage at "/" (priority 50)
    let user_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    // Specific writable mount at "/saves" (priority 10)
    let saves_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));

    vfs.mount(Mount::new(
        "mod_a",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "user_overlay",
        VfsPath::root(),
        50,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        user_mem,
    ))
    .unwrap();
    vfs.mount(Mount::new(
        "saves_mount",
        VfsPath::new("/saves").unwrap(),
        10,
        MountPolicy::ReadWrite,
        CaseSensitivity::Sensitive,
        saves_mem,
    ))
    .unwrap();

    // Writing to "/config.json" matches mod_a as the top candidate (priority 100 > 50).
    // Because mod_a is ReadOnly, the write is BLOCKED with ReadOnlyMount and must NOT mutate user_overlay.
    let write_err = vfs.write("/config.json", b"mutated").unwrap_err();
    assert_eq!(
        write_err,
        VfsError::ReadOnlyMount {
            mount_id: "mod_a".into(),
            path: VfsPath::new("/config.json").unwrap(),
        }
    );

    // Creating directory under read-only top mount is also blocked
    let mkdir_err = vfs.create_dir("/new_mod_dir").unwrap_err();
    assert_eq!(
        mkdir_err,
        VfsError::ReadOnlyMount {
            mount_id: "mod_a".into(),
            path: VfsPath::new("/new_mod_dir").unwrap(),
        }
    );

    // Writing to "/saves/save1.json" succeeds because prefix specificity (/saves vs /)
    // places saves_mount ahead of mod_a for that path prefix.
    vfs.write("/saves/save1.json", b"save_data").unwrap();
    assert_eq!(
        vfs.read_to_string("/saves/save1.json").unwrap(),
        "save_data"
    );
}

struct ErrorInjectingBackend {
    error_message: String,
}

impl crate::backend::VfsBackend for ErrorInjectingBackend {
    fn backend_type(&self) -> &'static str {
        "error_injector"
    }
    fn exists(&self, _rel_path: &str) -> bool {
        false
    }
    fn metadata(&self, _rel_path: &str) -> crate::error::VfsResult<crate::types::VfsMetadata> {
        Err(VfsError::AccessDenied(self.error_message.clone()))
    }
    fn read(&self, _rel_path: &str) -> crate::error::VfsResult<Vec<u8>> {
        Err(VfsError::AccessDenied(self.error_message.clone()))
    }
    fn write(&self, _rel_path: &str, _data: &[u8]) -> crate::error::VfsResult<()> {
        Err(VfsError::AccessDenied(self.error_message.clone()))
    }
    fn create_dir(&self, _rel_path: &str) -> crate::error::VfsResult<()> {
        Err(VfsError::AccessDenied(self.error_message.clone()))
    }
    fn read_dir(
        &self,
        _rel_path: &str,
    ) -> crate::error::VfsResult<Vec<crate::backend::BackendDirEntry>> {
        Err(VfsError::AccessDenied(self.error_message.clone()))
    }
}

#[test]
fn test_backend_error_propagation_not_swallowed() {
    let mut vfs = Vfs::new();

    let failing_backend = Arc::new(ErrorInjectingBackend {
        error_message: "disk hardware failure".into(),
    });

    let fallback_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    fallback_mem
        .set_file("file.txt", b"fallback_content")
        .unwrap();

    // Failing mount has higher priority (100)
    vfs.mount(Mount::new(
        "failing",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        failing_backend,
    ))
    .unwrap();

    // Fallback mount has lower priority (0)
    vfs.mount(Mount::new(
        "fallback",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        fallback_mem,
    ))
    .unwrap();

    // AccessDenied error MUST be propagated and must NOT fall through to fallback mount
    let read_err = vfs.read("/file.txt").unwrap_err();
    assert_eq!(
        read_err,
        VfsError::AccessDenied("disk hardware failure".into())
    );

    let meta_err = vfs.metadata("/file.txt").unwrap_err();
    assert_eq!(
        meta_err,
        VfsError::AccessDenied("disk hardware failure".into())
    );

    let readdir_err = vfs.read_dir("/").unwrap_err();
    assert_eq!(
        readdir_err,
        VfsError::AccessDenied("disk hardware failure".into())
    );
}

#[test]
fn test_case_insensitive_collision_determinism() {
    let mem = MemoryMount::new(CaseSensitivity::Insensitive);

    // Insert two entries that differ only in casing
    mem.set_file("items/item.DAT", b"lower_item").unwrap();
    mem.set_file("items/ITEM.dat", b"upper_item").unwrap();

    // Exact match for "item.DAT" returns "lower_item"
    assert_eq!(mem.read_to_string("items/item.DAT").unwrap(), "lower_item");

    // Exact match for "ITEM.dat" returns "upper_item"
    assert_eq!(mem.read_to_string("items/ITEM.dat").unwrap(), "upper_item");

    // Non-exact match for "Item.Dat" deterministically chooses the first in sorted lexicographical order
    // In ASCII sort: 'I' (uppercase) comes before 'i' (lowercase), so "ITEM.dat" is chosen.
    let resolved = mem.read_to_string("items/Item.Dat").unwrap();
    assert_eq!(resolved, "upper_item");
}

#[test]
fn test_physical_read_dir_error_propagation_on_non_directory() {
    let temp = std::env::temp_dir().join(format!("fu_vfs_non_dir_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();

    let file_path = temp.join("regular_file.txt");
    std::fs::write(&file_path, b"content").unwrap();

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "phys",
        "/",
        &temp,
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
    )
    .unwrap();

    // Calling read_dir on a regular file should return an error, not Ok(empty)
    let res = vfs.read_dir("/regular_file.txt");
    assert!(
        res.is_err(),
        "expected error when calling read_dir on a regular file"
    );

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn test_overlay_read_dir_case_insensitive_shadows_lower_mount() {
    let mut vfs = Vfs::new();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mod_mem
        .set_file("characters/character.xml", b"mod_content")
        .unwrap();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem
        .set_file("characters/CHARACTER.XML", b"base_content")
        .unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        mod_mem,
    ))
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

    // Directory listing: Higher-priority insensitive mount shadows all case variants
    let entries = vfs.read_dir("/characters").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "character.xml");
    assert_eq!(entries[0].file_type, VfsFileType::File);

    // Read accessibility: Both casings resolve to mod_content
    assert_eq!(
        vfs.read_to_string("/characters/character.xml").unwrap(),
        "mod_content"
    );
    assert_eq!(
        vfs.read_to_string("/characters/CHARACTER.XML").unwrap(),
        "mod_content"
    );
}

#[test]
fn test_overlay_read_dir_case_sensitive_preserves_reachable_lower_mount() {
    let mut vfs = Vfs::new();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem
        .set_file("characters/character.xml", b"mod_content")
        .unwrap();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    base_mem
        .set_file("characters/CHARACTER.XML", b"base_content")
        .unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "base",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        base_mem,
    ))
    .unwrap();

    // Directory listing: Higher sensitive mount does NOT shadow alternate casing in lower insensitive mount
    let entries = vfs.read_dir("/characters").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "CHARACTER.XML");
    assert_eq!(entries[1].name, "character.xml");

    // Read accessibility: Lower mount is reachable for its casing
    assert_eq!(
        vfs.read_to_string("/characters/character.xml").unwrap(),
        "mod_content"
    );
    assert_eq!(
        vfs.read_to_string("/characters/CHARACTER.XML").unwrap(),
        "base_content"
    );
}

#[test]
fn test_overlay_read_dir_exact_case_always_shadowed() {
    let mut vfs = Vfs::new();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem
        .set_file("characters/character.xml", b"mod_content")
        .unwrap();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem
        .set_file("characters/character.xml", b"base_content")
        .unwrap();

    vfs.mount(Mount::new(
        "mod",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
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

    let entries = vfs.read_dir("/characters").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "character.xml");
    assert_eq!(
        vfs.read_to_string("/characters/character.xml").unwrap(),
        "mod_content"
    );
}

#[test]
fn test_overlay_read_dir_child_mount_deduplication_and_case_sensitivity() {
    // 1. Direct child mount with same casing deduplicates root directory
    let mut vfs_exact = Vfs::new();

    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem.create_dir("stages").unwrap();

    let child_mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    child_mem.set_file("temple.xml", b"child_temple").unwrap();

    vfs_exact
        .mount(Mount::new(
            "root",
            VfsPath::root(),
            0,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            root_mem,
        ))
        .unwrap();

    vfs_exact
        .mount(Mount::new(
            "stages_child",
            VfsPath::new("/stages").unwrap(),
            50,
            MountPolicy::ReadOnly,
            CaseSensitivity::Insensitive,
            child_mem,
        ))
        .unwrap();

    // Exact name "stages" deduplicates to 1 entry
    let entries_exact = vfs_exact.read_dir("/").unwrap();
    assert_eq!(entries_exact.len(), 1);
    assert_eq!(entries_exact[0].name, "stages");
    assert_eq!(entries_exact[0].file_type, VfsFileType::Directory);

    // Read accessibility works through child mount
    assert_eq!(
        vfs_exact.read_to_string("/stages/temple.xml").unwrap(),
        "child_temple"
    );

    // 2. Child mount internal case sensitivity does NOT leak to parent directory level
    let mut vfs_diff = Vfs::new();

    let root_mem2 = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem2.create_dir("STAGES").unwrap();
    root_mem2
        .set_file("STAGES/castle.xml", b"root_castle")
        .unwrap();

    let child_mem2 = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    child_mem2.set_file("temple.xml", b"child_temple").unwrap();

    vfs_diff
        .mount(Mount::new(
            "root",
            VfsPath::root(),
            0,
            MountPolicy::ReadOnly,
            CaseSensitivity::Sensitive,
            root_mem2,
        ))
        .unwrap();

    vfs_diff
        .mount(Mount::new(
            "stages_child",
            VfsPath::new("/stages").unwrap(),
            50,
            MountPolicy::ReadOnly,
            CaseSensitivity::Insensitive,
            child_mem2,
        ))
        .unwrap();

    // Virtual mount point routing is exact, so both "STAGES" and "stages" are retained
    let entries_diff = vfs_diff.read_dir("/").unwrap();
    assert_eq!(entries_diff.len(), 2);
    assert_eq!(entries_diff[0].name, "STAGES");
    assert_eq!(entries_diff[1].name, "stages");

    // Both subdirectories are accessible according to their exact virtual paths
    assert_eq!(
        vfs_diff.read_to_string("/stages/temple.xml").unwrap(),
        "child_temple"
    );
    assert_eq!(
        vfs_diff.read_to_string("/STAGES/castle.xml").unwrap(),
        "root_castle"
    );
}

#[test]
fn test_higher_priority_root_file_vs_lower_priority_child_mount() {
    let mut vfs = Vfs::new();

    // Root mount with file "pack" (priority 100)
    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem.set_file("pack", b"file_payload").unwrap();

    // Child mount at "/pack/addon" (priority 0)
    let child_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    child_mem.set_file("addon.txt", b"addon").unwrap();

    vfs.mount(Mount::new(
        "root",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "child",
        VfsPath::new("/pack/addon").unwrap(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        child_mem,
    ))
    .unwrap();

    let entries = vfs.read_dir("/").unwrap();
    let pack_entry = entries.iter().find(|e| e.name == "pack").unwrap();

    // The authoritative File must NOT be replaced by an implicit Directory from lower submount
    assert_eq!(pack_entry.file_type, VfsFileType::File);
    assert_eq!(vfs.metadata("/pack").unwrap().file_type, VfsFileType::File);
    assert_eq!(vfs.read_to_string("/pack").unwrap(), "file_payload");
}

#[test]
fn test_lower_priority_root_file_vs_higher_priority_child_mount() {
    let mut vfs = Vfs::new();

    // Root mount `/` has priority 0 and contains a file named "pack"
    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem.set_file("pack", b"root_pack_file").unwrap();

    // Child mount `/pack/addon` has priority 100 and contains "addon.txt"
    let child_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    child_mem.set_file("addon.txt", b"addon_payload").unwrap();

    vfs.mount(Mount::new(
        "root",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "child",
        VfsPath::new("/pack/addon").unwrap(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        child_mem,
    ))
    .unwrap();

    // 1. read_dir("/") reports pack as File, not Directory
    let entries = vfs.read_dir("/").unwrap();
    let pack_entry = entries.iter().find(|e| e.name == "pack").unwrap();
    assert_eq!(pack_entry.file_type, VfsFileType::File);

    // 2. metadata("/pack") reports File
    assert_eq!(vfs.metadata("/pack").unwrap().file_type, VfsFileType::File);

    // 3. read("/pack") returns the root file's expected contents
    assert_eq!(vfs.read_to_string("/pack").unwrap(), "root_pack_file");

    // 4. read_dir("/pack") fails because /pack is a file
    assert!(vfs.read_dir("/pack").is_err());

    // 5. The child mount remains accessible through its correctly routed path
    assert_eq!(
        vfs.read_to_string("/pack/addon/addon.txt").unwrap(),
        "addon_payload"
    );
}

#[test]
fn test_higher_priority_child_mount_vs_lower_priority_root_directory() {
    let mut vfs = Vfs::new();

    // Root mount has directory "stages" with priority 0
    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem.create_dir("stages").unwrap();
    root_mem
        .set_file("stages/root_stage.xml", b"root_stage")
        .unwrap();

    // Direct child mount at "/stages" has priority 50
    let child_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    child_mem
        .set_file("child_stage.xml", b"child_stage")
        .unwrap();

    vfs.mount(Mount::new(
        "root",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "child",
        VfsPath::new("/stages").unwrap(),
        50,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        child_mem,
    ))
    .unwrap();

    let entries = vfs.read_dir("/").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "stages");
    assert_eq!(entries[0].file_type, VfsFileType::Directory);

    assert_eq!(
        vfs.metadata("/stages").unwrap().file_type,
        VfsFileType::Directory
    );
    assert_eq!(
        vfs.read_to_string("/stages/child_stage.xml").unwrap(),
        "child_stage"
    );
}

#[test]
fn test_nested_child_mount_ordering() {
    let mut vfs = Vfs::new();

    // Direct child mount at "/data" (priority 10)
    let direct_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    direct_mem.set_file("file.txt", b"direct").unwrap();

    // Nested child mount at "/data/stages/castle" (priority 100)
    let nested_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    nested_mem.set_file("castle.xml", b"nested").unwrap();

    vfs.mount(Mount::new(
        "direct",
        VfsPath::new("/data").unwrap(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        direct_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "nested",
        VfsPath::new("/data/stages/castle").unwrap(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        nested_mem,
    ))
    .unwrap();

    let entries = vfs.read_dir("/").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "data");
    assert_eq!(entries[0].file_type, VfsFileType::Directory);

    assert_eq!(
        vfs.metadata("/data").unwrap().file_type,
        VfsFileType::Directory
    );
    assert_eq!(vfs.read_to_string("/data/file.txt").unwrap(), "direct");
    assert_eq!(
        vfs.read_to_string("/data/stages/castle/castle.xml")
            .unwrap(),
        "nested"
    );
}

#[test]
fn test_case_insensitive_child_backend_with_differently_cased_root_directory() {
    let mut vfs = Vfs::new();

    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem
        .set_file("STAGES/root.txt", b"root_content")
        .unwrap();

    let child_mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    child_mem.set_file("child.txt", b"child_content").unwrap();

    vfs.mount(Mount::new(
        "root",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "child",
        VfsPath::new("/stages").unwrap(),
        50,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        child_mem,
    ))
    .unwrap();

    // read_dir("/") returns both entries
    let entries = vfs.read_dir("/").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "STAGES");
    assert_eq!(entries[1].name, "stages");

    // metadata agrees on both
    assert_eq!(
        vfs.metadata("/STAGES").unwrap().file_type,
        VfsFileType::Directory
    );
    assert_eq!(
        vfs.metadata("/stages").unwrap().file_type,
        VfsFileType::Directory
    );

    // read agrees and resolves distinct targets
    assert_eq!(
        vfs.read_to_string("/stages/child.txt").unwrap(),
        "child_content"
    );
    assert_eq!(
        vfs.read_to_string("/STAGES/root.txt").unwrap(),
        "root_content"
    );
}

#[test]
fn test_overlay_read_dir_alphabetical_order_preserved() {
    let mut vfs = Vfs::new();

    let mem_a = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem_a.set_file("zebra.png", b"z").unwrap();
    mem_a.set_file("apple.png", b"a").unwrap();

    let mem_b = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem_b.set_file("banana.png", b"b").unwrap();
    mem_b.set_file("Cherry.png", b"c").unwrap();

    vfs.mount(Mount::new(
        "a",
        VfsPath::root(),
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem_a,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "b",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem_b,
    ))
    .unwrap();

    let entries = vfs.read_dir("/").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    // ASCII sort order: 'C' (uppercase) < 'a' < 'b' < 'z'
    assert_eq!(
        names,
        vec!["Cherry.png", "apple.png", "banana.png", "zebra.png"]
    );
}

#[test]
fn test_read_with_provenance_highest_priority_mount() {
    let mut vfs = Vfs::new();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem
        .set_file("config.json", b"{\"source\":\"base\"}")
        .unwrap();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem
        .set_file("config.json", b"{\"source\":\"mod\"}")
        .unwrap();

    vfs.mount(Mount::new(
        "base_mount",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "mod_mount",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    let output = vfs.read_with_provenance("/config.json").unwrap();
    assert_eq!(output.mount_id, "mod_mount");
    assert_eq!(output.mount_priority, 100);
    assert_eq!(output.canonical_relative_path, "config.json");
    assert_eq!(output.bytes, b"{\"source\":\"mod\"}");
}

#[test]
fn test_read_with_provenance_fallback_on_not_found() {
    let mut vfs = Vfs::new();

    let base_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    base_mem.set_file("base_only.txt", b"base_data").unwrap();

    let mod_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mod_mem.set_file("mod_only.txt", b"mod_data").unwrap();

    vfs.mount(Mount::new(
        "base_mount",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        base_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "mod_mount",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mod_mem,
    ))
    .unwrap();

    let output = vfs.read_with_provenance("/base_only.txt").unwrap();
    assert_eq!(output.mount_id, "base_mount");
    assert_eq!(output.mount_priority, 0);
    assert_eq!(output.canonical_relative_path, "base_only.txt");
    assert_eq!(output.bytes, b"base_data");
}

#[test]
fn test_read_with_provenance_correct_mount_id_after_fallback() {
    let mut vfs = Vfs::new();

    let high_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    let mid_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mid_mem.set_file("shared/file.txt", b"mid_content").unwrap();
    let low_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    low_mem.set_file("shared/file.txt", b"low_content").unwrap();

    vfs.mount(Mount::new(
        "high",
        VfsPath::root(),
        200,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        high_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "mid",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mid_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "low",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        low_mem,
    ))
    .unwrap();

    let output = vfs.read_with_provenance("/shared/file.txt").unwrap();
    assert_eq!(output.mount_id, "mid");
    assert_eq!(output.mount_priority, 100);
    assert_eq!(output.canonical_relative_path, "shared/file.txt");
    assert_eq!(output.bytes, b"mid_content");
}

#[test]
fn test_read_with_provenance_non_not_found_stops_fallback() {
    let mut vfs = Vfs::new();

    let failing_backend = Arc::new(ErrorInjectingBackend {
        error_message: "io failure on drive".into(),
    });

    let fallback_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    fallback_mem.set_file("file.txt", b"fallback").unwrap();

    vfs.mount(Mount::new(
        "failing",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        failing_backend,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "fallback",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        fallback_mem,
    ))
    .unwrap();

    let err = vfs.read_with_provenance("/file.txt").unwrap_err();
    assert_eq!(err, VfsError::AccessDenied("io failure on drive".into()));
}

#[test]
fn test_read_with_provenance_directory_read_stops_fallback() {
    let mut vfs = Vfs::new();

    let high_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    high_mem
        .set_file("shared_path/child.txt", b"child")
        .unwrap();

    let low_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    low_mem.set_file("shared_path", b"file_content").unwrap();

    vfs.mount(Mount::new(
        "high",
        VfsPath::root(),
        100,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        high_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "low",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        low_mem,
    ))
    .unwrap();

    let err = vfs.read_with_provenance("/shared_path").unwrap_err();
    assert!(matches!(err, VfsError::Io { .. }));
}

#[test]
fn test_read_with_provenance_case_insensitive_entry_casing_memory() {
    let mut vfs = Vfs::new();

    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Insensitive));
    mem.set_file("Images/Characters/Boyfriend.PNG", b"bf_png")
        .unwrap();

    vfs.mount(Mount::new(
        "assets",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
        mem,
    ))
    .unwrap();

    let output = vfs
        .read_with_provenance("/images/characters/boyfriend.png")
        .unwrap();
    assert_eq!(output.mount_id, "assets");
    assert_eq!(
        output.canonical_relative_path,
        "Images/Characters/Boyfriend.PNG"
    );
    assert_eq!(output.bytes, b"bf_png");
}

#[test]
fn test_read_with_provenance_case_insensitive_entry_casing_physical() {
    let temp_dir = std::env::temp_dir().join(format!("fu_vfs_prov_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("Songs").join("Test")).unwrap();
    std::fs::write(
        temp_dir.join("Songs").join("Test").join("Chart.Json"),
        b"{\"notes\":[]}",
    )
    .unwrap();

    let mut vfs = Vfs::new();
    vfs.mount_physical(
        "phys_mount",
        "/data",
        &temp_dir,
        10,
        MountPolicy::ReadOnly,
        CaseSensitivity::Insensitive,
    )
    .unwrap();

    let output = vfs
        .read_with_provenance("/data/songs/test/chart.json")
        .unwrap();
    assert_eq!(output.mount_id, "phys_mount");
    assert_eq!(output.mount_priority, 10);
    assert_eq!(output.canonical_relative_path, "Songs/Test/Chart.Json");
    assert_eq!(output.bytes, b"{\"notes\":[]}");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_read_with_provenance_compatibility_with_read() {
    let mut vfs = Vfs::new();

    let mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    mem.set_file("audio/track.ogg", b"ogg_bytes").unwrap();

    vfs.mount(Mount::new(
        "mem",
        VfsPath::root(),
        0,
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        mem,
    ))
    .unwrap();

    let prov = vfs.read_with_provenance("/audio/track.ogg").unwrap();
    let bytes = vfs.read("/audio/track.ogg").unwrap();
    let text = vfs.read_to_string("/audio/track.ogg").unwrap();

    assert_eq!(prov.bytes, bytes);
    assert_eq!(bytes, b"ogg_bytes");
    assert_eq!(text, "ogg_bytes");

    assert!(matches!(
        vfs.read_with_provenance("/missing.txt"),
        Err(VfsError::NotFound(_))
    ));
    assert!(matches!(
        vfs.read("/missing.txt"),
        Err(VfsError::NotFound(_))
    ));
}

#[test]
fn test_read_with_provenance_nested_mount_points() {
    let mut vfs = Vfs::new();

    let root_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    root_mem
        .set_file("mods/demo/stage.json", b"root_stage")
        .unwrap();

    let nested_mem = Arc::new(MemoryMount::new(CaseSensitivity::Sensitive));
    nested_mem.set_file("stage.json", b"nested_stage").unwrap();

    vfs.mount(Mount::new(
        "root_mount",
        VfsPath::root(),
        100, // Higher priority but less specific prefix
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        root_mem,
    ))
    .unwrap();

    vfs.mount(Mount::new(
        "nested_mount",
        VfsPath::new("/mods/demo").unwrap(),
        0, // Lower priority but more specific prefix
        MountPolicy::ReadOnly,
        CaseSensitivity::Sensitive,
        nested_mem,
    ))
    .unwrap();

    let output = vfs.read_with_provenance("/mods/demo/stage.json").unwrap();
    assert_eq!(output.mount_id, "nested_mount");
    assert_eq!(output.mount_priority, 0);
    assert_eq!(output.canonical_relative_path, "stage.json");
    assert_eq!(output.bytes, b"nested_stage");
}
