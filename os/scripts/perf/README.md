# BuildStorm performance runner

`buildstorm_runner.py` runs one cold-image, snapshot-only measurement and stores
`serial.log` plus a machine-readable `result.json` below
`os/tem/perf/buildstorm/<run-id>/` by default.

WaterOS RISC-V example:

```bash
./scripts/perf/buildstorm_runner.py \
  --arch rv --kernel ./kernel-rv-final --image ./sdcard-rv-pub.img \
  --run-id wateros-rv-a1 --timeout 1800
```

Reference Linux kernels require the rootfs command line and guest test script:

```bash
./scripts/perf/buildstorm_runner.py \
  --arch rv --kernel ./kernel-rv-linux-baseline-6.12.102 \
  --image ./sdcard-rv-pub.img --run-id linux-rv-a1 --timeout 1200 \
  --linux-userland
```

Use `--plugin pc-hot` and/or `--plugin wait-hot` for diagnostic runs. Plugin
runs are marked `wall_clock_eligible: false` and must not be used for A/B wall
time acceptance. Use a 300 second timeout for the normal diagnostic snapshot.
Diagnostic kernels may emit one or more
`BUILDSTORM_PERF_COUNTERS key=value ...` lines; the runner records the latest
value for each integer counter in `result.json` under `perf_counters`.

Every run starts a fresh QEMU process with `-snapshot`. Before launch, the
runner calls `sync` and `posix_fadvise(POSIX_FADV_DONTNEED)` for the selected
image only. A run-id is never overwritten.
