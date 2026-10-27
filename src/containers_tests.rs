use super::*;
fn root() -> PathBuf {
    std::env::temp_dir().join(format!("wega-containers-{}", random_id().unwrap()))
}
#[test]
fn every_color_is_distinct_and_round_trips_through_custom_identities() {
    let root = root();
    let mut names = std::collections::HashSet::new();
    let mut colors = std::collections::HashSet::new();
    for color in Color::ALL {
        assert!(names.insert(color.name()));
        assert!(colors.insert(color.rgb()));
        let container = Container {
            id: format!("c-{}", random_id().unwrap()),
            name: color.name().into(),
            color,
        };
        Registry::update(&root, container.clone()).unwrap();
        assert_eq!(
            Registry::load(&root).unwrap().find(&container.id).unwrap(),
            &container
        );
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn defaults_migration_and_edits_preserve_identity_and_existing_data() {
    let root = root();
    let registry = Registry::load(&root).unwrap();
    assert_eq!(
        registry
            .containers
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Personal", "Work", "Banking", "Shopping"]
    );
    let work = profile_directory(&root, "work").unwrap();
    fs::write(work.join("cookie-fixture"), "private").unwrap();
    let mut renamed = registry.find("work").unwrap().clone();
    renamed.name = "仕事".into();
    renamed.color = Color::Purple;
    Registry::update(&root, renamed.clone()).unwrap();
    assert_eq!(
        Registry::load(&root).unwrap().find("work").unwrap(),
        &renamed
    );
    assert_eq!(profile_directory(&root, "work").unwrap(), work);
    let custom = Container {
        id: format!("c-{}", random_id().unwrap()),
        name: "Second account".into(),
        color: Color::Red,
    };
    Registry::update(&root, custom.clone()).unwrap();
    assert_eq!(Registry::load(&root).unwrap().containers.len(), 5);
    Registry::remove(&root, "work").unwrap();
    assert!(Registry::load(&root).unwrap().find("work").is_err());
    assert_eq!(
        fs::read_to_string(work.join("cookie-fixture")).unwrap(),
        "private"
    );
    assert!(!root.join("browser.json").exists());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn corrupt_registry_invalid_identity_and_concurrent_edit_fail_closed() {
    let root = root();
    fs::create_dir_all(&root).unwrap();
    let work = Registry::default().find("work").unwrap().clone();
    for bytes in [
        "broken",
        r#"{"version":2,"containers":[]}"#,
        r#"{"version":1,"containers":[{"id":"../main","name":"Bad","color":"Blue"}]}"#,
    ] {
        fs::write(root.join("containers.json"), bytes).unwrap();
        assert!(Registry::load(&root).is_err());
        assert!(Registry::update(&root, work.clone()).is_err());
        assert_eq!(
            fs::read_to_string(root.join("containers.json")).unwrap(),
            bytes
        );
    }
    fs::remove_file(root.join("containers.json")).unwrap();
    let mut bad = work.clone();
    bad.name = "\nInvisible".into();
    assert!(Registry::update(&root, bad).is_err());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("containers.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(Registry::update(&root, work).is_err());
    drop(lock);
    for id in ["", "..", "../work", "/work", "a/b", "a\\b", "a.b", "Work"] {
        assert!(profile_directory(&root, id).is_err(), "{id}");
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[cfg(unix)]
fn profile_directories_cannot_alias_the_default_or_another_container() {
    let root = root();
    fs::create_dir_all(root.join("containers")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("containers/work")).unwrap();
    assert!(profile_directory(&root, "work").is_err());
    fs::remove_file(root.join("containers/work")).unwrap();
    fs::remove_dir(root.join("containers")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("containers")).unwrap();
    assert!(profile_directory(&root, "personal").is_err());
    fs::remove_file(root.join("containers")).unwrap();
    fs::remove_dir_all(root).unwrap();
}
