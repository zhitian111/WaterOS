use crate::constants::*;
use crate::prelude::*;
use core::any::Any;

/// Interface for serializing and deserializing objects to and from bytes.
///
/// # Unsafe
///
/// This trait is unsafe because it allows arbitrary memory interpretation.
/// Implementor should guarantee the object is saved in the way defined by
/// functions `from_bytes` and `to_bytes`.
pub unsafe trait AsBytes
where
    Self: Sized,
{
    /// Default implementation that deserializes the object from a byte array.
    fn from_bytes(bytes: &[u8]) -> Self {
        unsafe { core::ptr::read(bytes.as_ptr() as *const Self) }
    }
    /// Default implementation that serializes the object to a byte array.
    fn to_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }
}

/// Common data block descriptor.
#[derive(Debug, Clone)]
pub struct Block {
    /// Physical block id
    pub id: PBlockId,
    /// Raw block data
    pub data: Arc<Box<[u8; BLOCK_SIZE]>>,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            id: 0,
            data: Arc::new(Box::new([0; BLOCK_SIZE])),
        }
    }
}

impl Block {
    /// Create new block with given physical block id and data.
    pub fn new(block_id: PBlockId, data: Box<[u8; BLOCK_SIZE]>) -> Self {
        Self {
            id: block_id,
            data: Arc::new(data),
        }
    }

    /// Borrow raw block data.
    pub fn data(&self) -> &[u8; BLOCK_SIZE] {
        self.data.as_ref().as_ref()
    }

    /// Get exclusive block data, copying shared cache data only on the first write.
    pub fn data_mut(&mut self) -> &mut [u8; BLOCK_SIZE] {
        Arc::make_mut(&mut self.data).as_mut()
    }

    /// Read `size` bytes from `offset` in block data.
    pub fn read_offset(&self, offset: usize, size: usize) -> &[u8] {
        &self.data()[offset..offset + size]
    }

    /// Read bytes from `offset` in block data and interpret it as `T`.
    pub fn read_offset_as<'a, T>(&self, offset: usize) -> T
    where
        T: AsBytes,
    {
        T::from_bytes(&self.data()[offset..])
    }

    /// Write block data to `offset` with `size`.
    pub fn write_offset(&mut self, offset: usize, data: &[u8]) {
        self.data_mut()[offset..offset + data.len()].copy_from_slice(data);
    }

    /// Transform `T` to bytes and write it to `offset`.
    pub fn write_offset_as<T>(&mut self, offset: usize, value: &T)
    where
        T: AsBytes,
    {
        self.write_offset(offset, value.to_bytes());
    }
}

/// Common interface for block devices.
pub trait BlockDevice: Send + Sync + Any {
    /// Read a block from disk.
    fn read_block(&self, block_id: PBlockId) -> Block;
    /// Read physically contiguous blocks into `buf`.
    ///
    /// Backends may override this to issue one device request. The default
    /// preserves compatibility with block-at-a-time implementations.
    fn read_blocks(&self, start_block: PBlockId, buf: &mut [u8]) {
        assert_eq!(buf.len() % BLOCK_SIZE, 0);
        for (index, chunk) in buf.chunks_exact_mut(BLOCK_SIZE).enumerate() {
            let block = self.read_block(start_block + index as PBlockId);
            chunk.copy_from_slice(block.data());
        }
    }
    /// Write a block to disk.
    fn write_block(&self, block: &Block);
}
