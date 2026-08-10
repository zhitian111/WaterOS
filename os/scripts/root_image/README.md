# WaterOS physical root image

This directory contains the rootless image builder intended for QEMU and real
SD-card/disk deployment. Its default layout is deliberately small:

- 32 MiB sparse raw disk;
- DOS/MBR disk id `0x574f5301` by default, or GPT via `--partition-table gpt`;
- one Linux partition starting at sector 2048 (1 MiB alignment);
- 4 KiB ext4 blocks, fixed UUID and label, no journal, with 64-bit descriptors;
- root contents declared by `rootfs-manifest.json`.

Build and independently verify an image from `os/`:

```bash
make physical-root-image ROOT_IMAGE=./wateros-root.img
make verify-physical-root-image ROOT_IMAGE=./wateros-root.img
# GPT layout (the default remains MBR for compatibility)
make physical-root-image ROOT_IMAGE=./wateros-gpt.img ROOT_PARTITION_TABLE=gpt
# inject a generated architecture root tree through the Make wrapper
make physical-root-image ROOT_IMAGE=./visionfive2-root.img \
  ROOT_PARTITION_TABLE=gpt ROOT_SOURCE_ROOT=./visionfive2-rootfs \
  ROOT_MANIFEST=./visionfive2-manifest.json
make verify-physical-root-image ROOT_IMAGE=./visionfive2-root.img \
  ROOT_SOURCE_ROOT=./visionfive2-rootfs \
  ROOT_MANIFEST=./visionfive2-manifest.json
```

The build requires `sfdisk`, `mkfs.ext4`, `e2fsck`, `dumpe2fs`, and `debugfs`.
It never mounts the image and does not require `sudo`. A replacement is built
and verified in the destination directory before atomically replacing an old
image. Use `--force` (the Make target does this) only when replacing the named
output is intended.

Each manifest file entry must contain an absolute guest `path`, an octal
`mode`, and exactly one of `content` or `source`. Relative sources are resolved
against the manifest directory. Add architecture-specific binaries through a
separate manifest rather than committing generated images.

For a generated architecture-specific root tree, pass
`--source-root ./rootfs` to both `build` and `verify`. In that mode every
manifest `source` must be relative to the supplied root and path traversal or
absolute host paths are rejected:

```bash
python3 scripts/root_image/root_image.py build --partition-table gpt \
  --source-root ./rootfs --manifest ./visionfive2-manifest.json \
  --output ./visionfive2-root.img --force
python3 scripts/root_image/root_image.py verify --source-root ./rootfs \
  --manifest ./visionfive2-manifest.json --image ./visionfive2-root.img
```

The fixed layout and metadata make builds predictable. Byte-for-byte identity
across different e2fsprogs versions is not guaranteed; release automation
should pin the host tool versions if that property becomes necessary.

The `64bit` ext4 feature is intentional even though the image is small: the
current vendored `another_ext4` backend fails to mount the legacy group
descriptor layout. QEMU regression tests cover this constraint. The journal is
disabled to keep the image small; WaterOS therefore does not yet promise
power-loss recovery for writes to this root filesystem.

The builder and verifier validate both MBR and GPT headers/entry arrays, including
CRC and partition bounds. If a GPT primary header is damaged, verification performs
one bounded read of the backup header at the end of the image, matching the kernel
block parser's fail-closed recovery behavior. The image has been exercised with QEMU virtio-blk. SD/eMMC controllers, cache
coherency, write barriers, flush semantics, and power-loss behavior still need
validation on each physical board.
