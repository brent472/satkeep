//! Where the backup file is kept: the owner's own iCloud or Google Drive.
//!
//! [`MemoryCloud`] stands in for both in tests. Like the real services, it
//! never truly forgets: deleted files and overwritten versions stay
//! retrievable (Drive keeps old revisions for about 30 days, iCloud Drive
//! has Recently Deleted, and anyone may have copied a file earlier). That's
//! what lets the tests show what replacing a device can and can't revoke.

/// The cloud folder the app uses for the backup.
pub trait CloudStore {
    /// Stores a new file and returns its id.
    fn write_new(&mut self, contents: Vec<u8>) -> u64;
    /// The ids of the files the app can currently see.
    fn list(&self) -> Vec<u64>;
    /// Reads a file the app can currently see.
    fn read(&self, id: u64) -> Option<Vec<u8>>;
    /// Deletes a file from the app's view.
    fn delete(&mut self, id: u64);
}

/// An in-memory cloud folder that keeps every file ever written, even after
/// it's deleted.
#[derive(Default)]
pub struct MemoryCloud {
    files: Vec<(u64, Vec<u8>, bool)>, // id, contents, visible
    next_id: u64,
}

impl MemoryCloud {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every file ever written, deleted or not: what an attacker with old
    /// revisions, the trash or an earlier copy could have.
    pub fn every_copy_ever(&self) -> Vec<Vec<u8>> {
        self.files.iter().map(|(_, contents, _)| contents.clone()).collect()
    }
}

impl CloudStore for MemoryCloud {
    fn write_new(&mut self, contents: Vec<u8>) -> u64 {
        self.next_id += 1;
        self.files.push((self.next_id, contents, true));
        self.next_id
    }

    fn list(&self) -> Vec<u64> {
        self.files.iter().filter(|(_, _, visible)| *visible).map(|(id, _, _)| *id).collect()
    }

    fn read(&self, id: u64) -> Option<Vec<u8>> {
        self.files.iter().find(|(i, _, visible)| *i == id && *visible).map(|(_, contents, _)| contents.clone())
    }

    fn delete(&mut self, id: u64) {
        for file in self.files.iter_mut().filter(|(i, _, _)| *i == id) {
            file.2 = false;
        }
    }
}
