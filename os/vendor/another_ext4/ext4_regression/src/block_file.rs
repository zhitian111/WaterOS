use another_ext4::{Block, BlockDevice, BLOCK_SIZE};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

pub struct BlockFile(File);

impl BlockFile {
    pub fn open(path: &str) -> Self {
        let file = OpenOptions::new().read(true).write(true).open(path).unwrap();
        Self(file)
    }
}

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: u64) -> Block {
        let mut file = &self.0;
        let mut buffer = [0u8; BLOCK_SIZE];
        file.seek(SeekFrom::Start(block_id * BLOCK_SIZE as u64)).unwrap();
        file.read_exact(&mut buffer).unwrap();
        Block::new(block_id, Box::new(buffer))
    }

    fn write_block(&self, block: &Block) {
        let mut file = &self.0;
        file.seek(SeekFrom::Start(block.id * BLOCK_SIZE as u64)).unwrap();
        file.write_all(&block.data()[..]).unwrap();
    }
}
