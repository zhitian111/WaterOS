# Another Ext4 File System in Rust

Rust implementation of ext4 file system. This file system is checked by Metis Model Checker.
See [here](https://github.com/LearningOS/osbiglab-2024s-fuzzingfilesystem) for details.

Initial version is forked from [ext4_rs](https://github.com/yuoo655/ext4_rs).

The WaterOS-maintained fork includes read/write integration fixes required by the
default kernel backend, including creation and reading of ext4 fast symlinks. Fast
symlink targets are stored in the inode's 60-byte `i_block` area; longer targets
are rejected rather than truncated.
