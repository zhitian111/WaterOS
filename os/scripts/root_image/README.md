# WaterOS physical root image

This directory contains the rootless image builder intended for QEMU and real
SD-card/disk deployment. Its default MBR layout is deliberately small:

- 32 MiB sparse raw disk;
- DOS/MBR disk id `0x574f5301`;
- one Linux partition (`0x83`) starting at sector 2048 (1 MiB alignment);
- 4 KiB ext4 blocks, fixed UUID and label, no journal, with 64-bit descriptors;
- root contents declared by `rootfs-manifest.json`.

Build and independently verify an image from `os/`:

```bash
make physical-root-image ROOT_IMAGE=./wateros-root.img
make verify-physical-root-image ROOT_IMAGE=./wateros-root.img

# 例如为某个架构使用独立清单，并保持小镜像用于测试
make physical-root-image ROOT_IMAGE=./wateros-rv64.img \
  ROOT_IMAGE_MANIFEST=./my-rootfs-manifest-rv64.json \
  ROOT_IMAGE_SIZE_MIB=16
make verify-physical-root-image ROOT_IMAGE=./wateros-rv64.img \
  ROOT_IMAGE_MANIFEST=./my-rootfs-manifest-rv64.json

# 如目标固件要求 GPT，也可使用小镜像先做 host/QEMU 验证
make physical-root-image ROOT_IMAGE=./wateros-gpt.img \
  ROOT_IMAGE_SIZE_MIB=16 ROOT_IMAGE_PARTITION_TABLE=gpt
make verify-physical-root-image ROOT_IMAGE=./wateros-gpt.img

# 可选第二个 ext4 数据分区（root manifest 写入 /dev/vda1，data manifest 写入 /dev/vda2）
make physical-root-image ROOT_IMAGE=./wateros-with-data.img \
  ROOT_IMAGE_SIZE_MIB=16 ROOT_IMAGE_DATA_SIZE_MIB=4 \
  ROOT_IMAGE_DATA_MANIFEST=./data-manifest.json
make verify-physical-root-image ROOT_IMAGE=./wateros-with-data.img \
  ROOT_IMAGE_DATA_MANIFEST=./data-manifest.json
```

The build requires `sfdisk`, `mkfs.ext4`, `e2fsck`, `dumpe2fs`, and `debugfs`.
It never mounts the image and does not require `sudo`. A replacement is built
and verified in the destination directory before atomically replacing an old
image. Use `--force` (the Make target does this) only when replacing the named
output is intended.

`ROOT_IMAGE_MANIFEST` is passed to both build and verify, so the same
architecture-specific manifest is checked after construction. Set
`ROOT_IMAGE_DATA_MANIFEST` together with `ROOT_IMAGE_DATA_SIZE_MIB` to create and
verify a second Linux/ext4 data partition; leaving it unset preserves the
single-root-partition layout. `ROOT_IMAGE_SIZE_MIB`
defaults to 32 and may be reduced to 16 for small host/QEMU tests (the tool
still enforces a 1 MiB-aligned partition and ext4 metadata minimum). The
`ROOT_IMAGE_PARTITION_TABLE` Make variable defaults to `mbr`; `gpt` emits a
protective MBR, primary/backup GPT metadata and a CRC-checked Linux root
partition. Verification also checks that the backup header and entry array
match the primary metadata.

The data partition is deliberately not auto-mounted by the kernel. After the
root volume is available, userspace may mount it explicitly, for example:

```sh
mkdir -p /data
mount -t ext4 /dev/vda2 /data
```

The VFS bridge routes this through `mount_ext4_block_at`, validates that the
target is an empty directory, keeps the root handle separate, and supports
`MS_RDONLY`. A platform-specific auto-mount policy can call the same API later;
absence or mount failure must remain non-fatal for single-partition images.

Each manifest file entry must contain an absolute guest `path`, an octal
`mode`, and exactly one of `content` or `source`. Relative sources are resolved
against the manifest directory. Add architecture-specific binaries through a
separate manifest rather than committing generated images.

The fixed layout and metadata make builds predictable. Byte-for-byte identity
across different e2fsprogs versions is not guaranteed; release automation
should pin the host tool versions if that property becomes necessary.

The `64bit` ext4 feature is intentional even though the image is small: the
current vendored `another_ext4` backend fails to mount the legacy group
descriptor layout. QEMU regression tests cover this constraint. The journal is
disabled to keep the image small; WaterOS therefore does not yet promise
power-loss recovery for writes to this root filesystem.

The image has been exercised with QEMU virtio-blk. SD/eMMC controllers, cache
coherency, write barriers, flush semantics, and power-loss behavior still need
validation on each physical board.

After building a kernel artifact, the snapshot smoke helper validates the same
manifest and prints the exact QEMU command without starting a guest:

```bash
python3 ./scripts/root_image/qemu_smoke.py \
  --arch rv --profile pre --image ./wateros-root.img \
  --manifest ./scripts/root_image/rootfs-manifest.json \
  --kernel ./kernel-rv-pre
```

Add `--execute` to run QEMU with `-snapshot`; a timeout or missing kernel/QEMU
is reported as a smoke failure. Add `--require-root-mount` when the run must
also emit a recognized ext4 root-mount line; absent and failed mount evidence
are rejected. The guest's physical SD/eMMC behavior remains
`UNVERIFIED_ON_HARDWARE`.
