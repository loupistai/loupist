use super::*;

pub(super) fn loupist() -> LoupistProfile {
    loupist_with_root("loupist-1", 11, 22)
}

pub(super) fn loupist_with_root(id: &str, device: u64, inode: u64) -> LoupistProfile {
    LoupistProfile {
        name: format!("{id} Loupist"),
        root_filesystem_identity: Some(RootFilesystemIdentity { device, inode }),
        ..crate::test_fixtures::loupist_profile(id)
    }
}

pub(super) fn create_loupist(store: &SqliteStore, profile: &LoupistProfile) -> StoreResult<()> {
    store.create_loupist(profile)
}

pub(super) fn seed_loupist(store: &SqliteStore, profile: &LoupistProfile) {
    create_loupist(store, profile).unwrap();
}
