#!/bin/sh
# BuildStorm testcode -- runs INSIDE the guest, on the kernel under test.
# The guest image is a self-contained rootfs (Debian glibc + rust toolchain +
# tgoskits sources + cargo cache); the student mounts it as their rootfs.
# Usage: sh /buildstorm_testcode.sh
#
# Emits (parsed by judge/judge_buildstorm-glibc.py):
#   TOOLCHAIN_RESULT status=OK|FAIL                          (8 pts)
#   MINIBUILD_RESULT status=OK|FAIL                          (12 pts)
#   BUILDSTORM_RESULT mode=multi status=OK|FAIL rc=<n> cores=<n> elapsed_s=<s> [artifact=<p> bytes=<n>]
#                                                            (40 + 120 pts)
# Wrapped in official OS COMP TEST GROUP markers. Group is fixed buildstorm-glibc
# (this rootfs is glibc-based); autotest matches it to judge_buildstorm-glibc.py.

echo "#### OS COMP TEST GROUP START buildstorm-glibc ####"

mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null
mount -t devtmpfs devtmpfs /dev 2>/dev/null

export PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/sbin:/usr/sbin
export HOME=/root RUSTUP_HOME=/root/.rustup CARGO_HOME=/root/.cargo
export RUSTUP_TOOLCHAIN=nightly-2026-05-28
export CARGO_NET_OFFLINE=true

# arch-adaptive: build arceos for the guest's OWN arch (native), not hardcoded.
case "$(uname -m 2>/dev/null)" in
  loongarch64) AXARCH=loongarch64; AXTGT=loongarch64-unknown-linux-musl ;;
  riscv64)     AXARCH=riscv64;     AXTGT=riscv64gc-unknown-linux-musl ;;
  *)           AXARCH=riscv64;     AXTGT=riscv64gc-unknown-linux-musl ;;
esac

# ---------- toolchain can run (dynamic linking / glibc / basic syscalls)
if rustc --version && cargo --version; then
    echo "TOOLCHAIN_RESULT status=OK"
else
    echo "TOOLCHAIN_RESULT status=FAIL"
fi

# ---------- minimal cargo project: full dep-resolve -> codegen -> link -> run
rm -rf /tmp/minibuild
if cargo new --vcs none /tmp/minibuild >/dev/null 2>&1 \
   && ( cd /tmp/minibuild && cargo build >/dev/null 2>&1 ) \
   && [ "$(/tmp/minibuild/target/debug/minibuild)" = "Hello, world!" ]; then
    echo "MINIBUILD_RESULT status=OK"
else
    echo "MINIBUILD_RESULT status=FAIL"
fi

# ---------- compile arceos-helloworld from scratch, timed
cd /work/tgoskits 2>/dev/null || {
    echo "BUILDSTORM_RESULT mode=multi status=FAIL rc=127 cores=$(nproc) elapsed_s=0"
    echo "#### OS COMP TEST GROUP END buildstorm-glibc ####"
    exit 1
}

# from-scratch guarantee: drop previous arceos artifacts for THIS arch
rm -rf "target/$AXTGT"

# pre-build the xtask helper OUTSIDE the timed window
echo "----- pre-build tg-xtask (untimed) -----"
cargo build -p tg-xtask 2>&1 || true

# Timed build. elapsed_s is measured in-guest via /proc/uptime (guarded so a
# kernel without /proc/uptime still prints a valid 0). Tampering with the clock
# or /proc/uptime is treated as cheating (contest rule).
echo "----- build arceos-helloworld (timed, arch=$AXARCH) -----"
echo "BUILDSTORM_BEGIN mode=multi"
T0=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
{ timeout 14400 cargo xtask arceos build -p arceos-helloworld --arch "$AXARCH" 2>&1; \
  echo $? > /work/.build.rc; } | tee /work/buildstorm.build.out
RC=$(cat /work/.build.rc 2>/dev/null || echo 1); rm -f /work/.build.rc
T1=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
ELAPSED=$(awk "BEGIN{printf \"%.2f\", (\"$T1\"+0)-(\"$T0\"+0)}" 2>/dev/null); [ -z "$ELAPSED" ] && ELAPSED=0
ART=$(find target -type f \( -name 'arceos-helloworld' -o -name 'helloworld' \) 2>/dev/null | head -1)
BYTES=0
[ -n "$ART" ] && BYTES=$(wc -c <"$ART")

# ---------- boot the compiled ArceOS kernel in qemu (compile-correctness, UNTIMED)
# arceos-helloworld is a bare-metal ArceOS unikernel -- it must be BOOTED in
# qemu, not exec'd. Runs AFTER T1, so never counted in elapsed_s.
#   - RISC-V   : an ELF; booted via OpenSBI (-bios) + -kernel.
#   - LoongArch: built as a PE32+ EFI app (.bin); booted via UEFI = edk2 pflash
#                (code.fd + vars.fd) + a FAT ESP holding BOOTLOONGARCH64.EFI.
#                (direct -kernel does NOT work for the LA EFI app.)
# Uses the image-bundled qemu under /opt/qemu-rv64 or /opt/qemu-la64.
rm -f /work/buildstorm.run.out
RUN_OK=0
if [ "$RC" -eq 0 ] && [ -n "$ART" ] && [ "$BYTES" -ge 500000 ]; then
    echo "----- boot arceos-helloworld in qemu (untimed, arch=$AXARCH) -----"
    : > /work/buildstorm.run.out

    if [ "$AXARCH" = "loongarch64" ]; then
        # ---- LoongArch: UEFI boot of the EFI app via pflash edk2 + ESP ----
        EFI="${ART}.bin"
        QROOT=/opt/qemu-la64
        QEMU_LD="$QROOT/lib/ld-linux-loongarch-lp64d.so.1"
        QEMU_BIN="$QROOT/bin/qemu-system-loongarch64"
        QEMU_CODE="$QROOT/share/edk2/loongarch64/code.fd"
        QEMU_VARS="$QROOT/share/edk2/loongarch64/vars.fd"

        if [ -f "$EFI" ] && [ -x "$QEMU_LD" ] && [ -x "$QEMU_BIN" ] && [ -f "$QEMU_CODE" ] && [ -f "$QEMU_VARS" ]; then
            rm -rf /work/buildstorm.esp
            mkdir -p /work/buildstorm.esp/EFI/BOOT
            cp "$EFI" /work/buildstorm.esp/EFI/BOOT/BOOTLOONGARCH64.EFI
            cp "$QEMU_VARS" /work/buildstorm.vars.fd
            "$QEMU_LD" --library-path "$QROOT/lib" "$QEMU_BIN" \
                -L "$QROOT/share/qemu" \
                -machine virt -cpu la464 -smp 1 -m 2G -nographic -serial mon:stdio \
                -drive if=pflash,format=raw,unit=0,readonly=on,file="$QEMU_CODE" \
                -drive if=pflash,format=raw,unit=1,file=/work/buildstorm.vars.fd \
                -drive format=raw,file=fat:rw:/work/buildstorm.esp \
                > /work/buildstorm.run.out 2>&1 &
            QPID=$!
            i=0
            while [ "$i" -lt 600 ]; do
                grep -qi "hello, world" /work/buildstorm.run.out 2>/dev/null && { RUN_OK=1; break; }
                kill -0 "$QPID" 2>/dev/null || break
                sleep 1; i=$((i+1))
            done
            kill "$QPID" 2>/dev/null; wait "$QPID" 2>/dev/null
        fi
    else
        # ---- RISC-V: OpenSBI firmware + direct -kernel of the ELF ----
        QEMU_LD=/opt/qemu-rv64/lib/ld-linux-riscv64-lp64d.so.1
        QEMU_BIN=/opt/qemu-rv64/bin/qemu-system-riscv64
        QEMU_BIOS=/opt/qemu-rv64/share/opensbi-riscv64-generic-fw_dynamic.bin

        if [ -x "$QEMU_BIN" ] && [ -x "$QEMU_LD" ] && [ -r "$QEMU_BIOS" ]; then
            "$QEMU_LD" --library-path "$(dirname "$QEMU_LD")" "$QEMU_BIN" \
                -machine virt -smp 1 -m 256M -nographic -bios "$QEMU_BIOS" -kernel "$ART" \
                > /work/buildstorm.run.out 2>&1 &
            QPID=$!
            i=0
            while [ "$i" -lt 300 ]; do
                grep -qi "hello, world" /work/buildstorm.run.out 2>/dev/null && { RUN_OK=1; break; }
                kill -0 "$QPID" 2>/dev/null || break
                sleep 1; i=$((i+1))
            done
            kill "$QPID" 2>/dev/null; wait "$QPID" 2>/dev/null
        fi
    fi
fi

echo "----- buildstorm.run.out -----"
cat /work/buildstorm.run.out 2>/dev/null

# success = rc 0 + artifact exists + plausible size + boots to Hello, world!
if [ "$RC" -eq 0 ] && [ -n "$ART" ] && [ "$BYTES" -ge 500000 ] && [ "$RUN_OK" -eq 1 ]; then
    echo "BUILDSTORM_RESULT mode=multi status=OK rc=$RC cores=$(nproc) elapsed_s=$ELAPSED artifact=$ART bytes=$BYTES run=OK"
else
    echo "BUILDSTORM_RESULT mode=multi status=FAIL rc=$RC cores=$(nproc) elapsed_s=$ELAPSED run=$([ "$RUN_OK" = 1 ] && echo OK || echo FAIL)"
    echo "----- buildstorm.build.out tail -----"
    tail -25 /work/buildstorm.build.out 2>/dev/null
    [ -f /work/buildstorm.run.out ] && { echo "----- buildstorm.run.out tail -----"; tail -15 /work/buildstorm.run.out 2>/dev/null; }
fi

echo "#### OS COMP TEST GROUP END buildstorm-glibc ####"
sync
