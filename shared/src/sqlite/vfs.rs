//! Read-only SQLite VFS that lets SQLite read a shard's bytes from an in-memory buffer: the wasm32
//! counterpart to the non-wasm32 path's rusqlite `deserialize`. Read-only because shards are
//! immutable. Built on `sqlite-wasm-rs`'s `SQLiteVfs` framework.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sqlite_wasm_rs::utils::{
    SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};
use sqlite_wasm_rs::{sqlite3_vfs, sqlite3_vfs_find, sqlite3_vfs_register, SQLITE_IOERR, SQLITE_OK, SQLITE_READONLY};

pub(crate) const VFS_NAME: &CStr = c"eafora-shard-ro";

struct ReadOnlyShardFile {
    bytes: Arc<Vec<u8>>,
}

impl VfsFile for ReadOnlyShardFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        let size: usize = self.bytes.len();
        if offset >= size {
            buf.fill(0);
            return Ok(false);
        }

        let available: usize = size - offset;
        let copy_len: usize = buf.len().min(available);
        buf[..copy_len].copy_from_slice(&self.bytes[offset..offset + copy_len]);

        if copy_len < buf.len() {
            buf[copy_len..].fill(0);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    fn write(&mut self, _buf: &[u8], _offset: usize) -> VfsResult<()> {
        Err(VfsError::new(SQLITE_READONLY, "eafora shard VFS is read-only".into()))
    }

    fn truncate(&mut self, _size: usize) -> VfsResult<()> {
        Err(VfsError::new(SQLITE_READONLY, "eafora shard VFS is read-only".into()))
    }

    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self.bytes.len())
    }
}

type ShardAppData = RefCell<HashMap<String, ReadOnlyShardFile>>;

struct ShardStore;

impl VfsStore<ReadOnlyShardFile, ShardAppData> for ShardStore {
    fn add_file(_vfs: *mut sqlite3_vfs, file: &str, _flags: i32) -> VfsResult<()> {
        // Unreachable: shards are pre-registered by `open_shard` and opened READONLY (no CREATE),
        // so xOpen finds the existing file and never asks the store to create one.
        Err(VfsError::new(SQLITE_READONLY, format!("cannot create {file} in the read-only shard VFS")))
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let app_data: &VfsAppData<ShardAppData> = unsafe { Self::app_data(vfs) };
        Ok(app_data.borrow().contains_key(file))
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let app_data: &VfsAppData<ShardAppData> = unsafe { Self::app_data(vfs) };
        app_data.borrow_mut().remove(file);
        Ok(())
    }

    fn with_file<F: Fn(&ReadOnlyShardFile) -> VfsResult<i32>>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name: &str = unsafe { vfs_file.name() };
        let app_data: &VfsAppData<ShardAppData> = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.borrow().get(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut ReadOnlyShardFile) -> VfsResult<i32>>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name: &str = unsafe { vfs_file.name() };
        let app_data: &VfsAppData<ShardAppData> = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.borrow_mut().get_mut(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }
}

struct ShardIoMethods;

impl SQLiteIoMethods for ShardIoMethods {
    type File = ReadOnlyShardFile;
    type AppData = ShardAppData;
    type Store = ShardStore;

    const VERSION: std::os::raw::c_int = 1;
}

struct ShardVfs;

impl SQLiteVfs<ShardIoMethods> for ShardVfs {
    const VERSION: std::os::raw::c_int = 1;
}

/// Register the VFS once (idempotent) and return its app-data map for pre-populating shard bytes.
fn install() -> &'static VfsAppData<ShardAppData> {
    let existing: *mut sqlite3_vfs = unsafe { sqlite3_vfs_find(VFS_NAME.as_ptr()) };

    let vfs: *mut sqlite3_vfs = if existing.is_null() {
        let vfs: &mut sqlite3_vfs = Box::leak(Box::new(ShardVfs::vfs(
            VFS_NAME.as_ptr(),
            VfsAppData::new(ShardAppData::default()).leak(),
        )));
        let register_rc: std::os::raw::c_int = unsafe { sqlite3_vfs_register(vfs, 0) };
        assert_eq!(register_rc, SQLITE_OK, "failed to register the eafora shard VFS");
        vfs as *mut sqlite3_vfs
    } else {
        existing
    };

    unsafe { ShardStore::app_data(vfs) }
}

/// Register `bytes` under a unique filename so SQLite can open it read-only via [`VFS_NAME`].
/// Pair every call with [`unregister_shard`] after the connection closes.
pub(crate) fn register_shard(bytes: &[u8]) -> String {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    let filename: String = format!("shard-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let app_data: &VfsAppData<ShardAppData> = install();
    app_data
        .borrow_mut()
        .insert(filename.clone(), ReadOnlyShardFile { bytes: Arc::new(bytes.to_vec()) });

    filename
}

pub(crate) fn unregister_shard(filename: &str) {
    let app_data: &VfsAppData<ShardAppData> = install();
    app_data.borrow_mut().remove(filename);
}
