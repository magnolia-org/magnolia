/*
A storage middleware that caches pieces in memory, so that subsequent reads (for checksumming) are
free.

An example, untested and unproven to be useful.
*/

use std::num::NonZeroUsize;

use anyhow::Context;
use librqbit_core::lengths::{Lengths, ValidPieceIndex};
use lru::LruCache;
use parking_lot::RwLock;

use crate::{
    FileInfos, ManagedTorrentShared,
    storage::{StorageFactory, StorageFactoryExt, TorrentStorage},
    torrent_state::TorrentMetadata,
};

#[derive(Clone, Copy)]
pub struct WriteThroughCacheStorageFactory<U> {
    max_cache_bytes: u64,
    underlying: U,
}

impl<U> WriteThroughCacheStorageFactory<U> {
    pub fn new(max_cache_bytes: u64, underlying: U) -> Self {
        Self {
            max_cache_bytes,
            underlying,
        }
    }
}

impl<U: StorageFactory + Clone> StorageFactory for WriteThroughCacheStorageFactory<U> {
    type Storage = WriteThroughCacheStorage<U::Storage>;

    fn create(
        &self,
        shared: &crate::ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        let pieces = self
            .max_cache_bytes
            .div_ceil(metadata.info.lengths().default_piece_length() as u64)
            .try_into()?;
        let pieces = NonZeroUsize::new(pieces).context("bug: pieces == 0")?;
        let lru = RwLock::new(LruCache::new(pieces));
        Ok(WriteThroughCacheStorage {
            lru,
            underlying: self.underlying.create(shared, metadata)?,
            lengths: *metadata.info.lengths(),
            file_infos: metadata.file_infos.clone(),
        })
    }

    fn clone_box(&self) -> crate::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

pub struct WriteThroughCacheStorage<U> {
    lru: RwLock<LruCache<ValidPieceIndex, Box<[u8]>>>,
    lengths: Lengths,
    file_infos: FileInfos,
    underlying: U,
}

impl<U> WriteThroughCacheStorage<U> {
    // magnolia: the piece and the offset inside it for a position in a file. upstream used
    // `Lengths::compute_current_piece`, whose `piece_offset` is the piece index in
    // librqbit-core 9.0.1 (abs / piece_len instead of abs % piece_len) and which rejects
    // larger positions, so cached chunks landed in the wrong place and every hash failed.
    fn locate(&self, file_id: usize, offset: u64) -> anyhow::Result<(ValidPieceIndex, usize)> {
        let file = self.file_infos.get(file_id).context("wrong file")?;
        let abs = file.offset_in_torrent + offset;
        let dpl = self.lengths.default_piece_length() as u64;
        let id = self
            .lengths
            .validate_piece_index((abs / dpl).try_into()?)
            .context("wrong piece")?;
        Ok((id, (abs % dpl) as usize))
    }
}

impl<U: TorrentStorage> TorrentStorage for WriteThroughCacheStorage<U> {
    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let (id, start) = self.locate(file_id, offset)?;
        let mut g = self.lru.write();
        if let Some(p) = g.get(&id) {
            let end = start + buf.len();
            let pbuf = p.get(start..end).context("bugged length")?;
            buf.copy_from_slice(pbuf);
            return Ok(());
        }
        self.underlying.pread_exact(file_id, offset, buf)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        // magnolia: the default vectored write passes its empty second slice too, positioned
        // at the end of the file, which is past the last piece
        if buf.is_empty() {
            return Ok(());
        }
        let (id, start) = self.locate(file_id, offset)?;
        let mut g = self.lru.write();
        let pbuf = g.get_or_insert_mut(id, || {
            vec![0; self.lengths.piece_length(id) as usize].into_boxed_slice()
        });
        let end = start + buf.len();
        pbuf.get_mut(start..end)
            .context("bugged range")?
            .copy_from_slice(buf);
        self.underlying.pwrite_all(file_id, offset, buf)
    }

    fn remove_file(&self, file_id: usize, filename: &std::path::Path) -> anyhow::Result<()> {
        self.underlying.remove_file(file_id, filename)
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.underlying.ensure_file_length(file_id, length)
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let replacement_cache = LruCache::new(NonZeroUsize::new(1).context("unreachable")?);
        let lru = std::mem::replace(&mut *self.lru.write(), replacement_cache);
        Ok(Box::new(WriteThroughCacheStorage {
            lru: RwLock::new(lru),
            underlying: self.underlying.take()?,
            lengths: self.lengths,
            file_infos: self.file_infos.clone(),
        }))
    }

    fn remove_directory_if_empty(&self, path: &std::path::Path) -> anyhow::Result<()> {
        self.underlying.remove_directory_if_empty(path)
    }

    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.underlying.init(shared, metadata)
    }
}
