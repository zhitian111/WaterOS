# Another Ext4 File System in Rust

Rust implementation of ext4 file system. This file system is checked by Metis Model Checker.
See [here](https://github.com/LearningOS/osbiglab-2024s-fuzzingfilesystem) for details.

Initial version is forked from [ext4_rs](https://github.com/yuoo655/ext4_rs).

The WaterOS-maintained fork includes read/write integration fixes required by the
default kernel backend, including creation and reading of ext4 symbolic links.
Targets up to 60 bytes use the inode's inline `i_block` area; longer targets are
stored in ordinary extents and are never truncated.
