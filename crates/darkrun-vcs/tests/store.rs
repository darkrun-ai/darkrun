//! Credential store roundtrip / remove / permission tests.

use darkrun_vcs::{Credential, CredentialStore, Provider};

fn temp_store() -> (tempfile::TempDir, CredentialStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::at(dir.path().join(".darkrun").join("credentials"));
    (dir, store)
}

#[test]
fn save_then_get_roundtrips() {
    let (_dir, store) = temp_store();
    let cred = Credential::new(Provider::GitHub, "gho_token");
    store.save(&cred).unwrap();

    let loaded = store.get(Provider::GitHub).unwrap().expect("present");
    assert_eq!(loaded, cred);
}

#[test]
fn get_absent_provider_is_none() {
    let (_dir, store) = temp_store();
    assert!(store.get(Provider::GitLab).unwrap().is_none());
}

#[test]
fn save_two_providers_keeps_both() {
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "gh")).unwrap();
    store.save(&Credential::new(Provider::GitLab, "gl")).unwrap();

    assert_eq!(store.get(Provider::GitHub).unwrap().unwrap().access_token, "gh");
    assert_eq!(store.get(Provider::GitLab).unwrap().unwrap().access_token, "gl");

    let mut providers = store.list().unwrap();
    providers.sort_by_key(|p| p.key());
    assert_eq!(providers, vec![Provider::GitHub, Provider::GitLab]);
}

#[test]
fn save_same_provider_replaces() {
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "old")).unwrap();
    store.save(&Credential::new(Provider::GitHub, "new")).unwrap();
    assert_eq!(store.get(Provider::GitHub).unwrap().unwrap().access_token, "new");
}

#[test]
fn remove_deletes_and_reports() {
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "t")).unwrap();

    assert!(store.remove(Provider::GitHub).unwrap());
    assert!(store.get(Provider::GitHub).unwrap().is_none());
    // Removing again returns false.
    assert!(!store.remove(Provider::GitHub).unwrap());
}

#[test]
fn remove_leaves_other_providers_intact() {
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "gh")).unwrap();
    store.save(&Credential::new(Provider::GitLab, "gl")).unwrap();

    store.remove(Provider::GitHub).unwrap();
    assert!(store.get(Provider::GitHub).unwrap().is_none());
    assert_eq!(store.get(Provider::GitLab).unwrap().unwrap().access_token, "gl");
}

#[test]
fn roundtrips_optional_fields() {
    let (_dir, store) = temp_store();
    let cred = Credential {
        provider: Provider::GitLab,
        access_token: "glpat".into(),
        refresh_token: Some("refresh".into()),
        expires_in: Some(7200),
        token_type: Some("bearer".into()),
    };
    store.save(&cred).unwrap();
    assert_eq!(store.get(Provider::GitLab).unwrap().unwrap(), cred);
}

#[cfg(unix)]
#[test]
fn credentials_file_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "secret")).unwrap();

    let meta = std::fs::metadata(store.path()).unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "credentials file must be 0600, got {mode:o}");
}

#[cfg(unix)]
#[test]
fn mode_stays_0600_after_overwrite() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, store) = temp_store();
    store.save(&Credential::new(Provider::GitHub, "a")).unwrap();
    // Loosen it, then save again — store must re-enforce 0600.
    let loose = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(store.path(), loose).unwrap();
    store.save(&Credential::new(Provider::GitLab, "b")).unwrap();

    let mode = std::fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn creates_parent_directory_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("deeply").join("nested").join("credentials");
    let store = CredentialStore::at(&nested);
    store.save(&Credential::new(Provider::GitHub, "t")).unwrap();
    assert!(nested.exists());
}

#[test]
fn empty_file_is_treated_as_empty_map() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials");
    std::fs::write(&path, b"").unwrap();
    let store = CredentialStore::at(&path);
    assert!(store.get(Provider::GitHub).unwrap().is_none());
    assert!(store.list().unwrap().is_empty());
}
