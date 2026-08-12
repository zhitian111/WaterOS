#!/bin/sh
# Local diagnostic: boot the BuildStorm artifact already present in this
# writable image.  No Cargo work is performed and nested QEMU output is
# streamed to the outer console while also being retained in /work.

echo "#### OS COMP TEST GROUP START buildstorm-qemu-only ####"

case "$(uname -m 2>/dev/null)" in
    loongarch64)
        ARCH=loongarch64
        ART=/work/tgoskits/target/loongarch64-unknown-linux-musl/release/arceos-helloworld
        ;;
    *)
        ARCH=riscv64
        ART=/work/tgoskits/target/riscv64gc-unknown-linux-musl/release/arceos-helloworld
        ;;
esac

BYTES=0
[ -f "$ART" ] && BYTES=$(wc -c <"$ART")
echo "QEMU_ONLY_BEGIN arch=$ARCH artifact=$ART bytes=$BYTES"

RUN_OUT=/work/buildstorm.qemu-only.run.out
: >"$RUN_OUT"

if [ "$BYTES" -ge 500000 ] && [ "$ARCH" = loongarch64 ]; then
    EFI="${ART}.bin"
    QROOT=/opt/qemu-la64
    QEMU_LD="$QROOT/lib/ld-linux-loongarch-lp64d.so.1"
    QEMU_BIN="$QROOT/bin/qemu-system-loongarch64"
    QEMU_CODE="$QROOT/share/edk2/loongarch64/code.fd"
    QEMU_VARS="$QROOT/share/edk2/loongarch64/vars.fd"
    rm -rf /work/buildstorm.qemu-only.esp
    mkdir -p /work/buildstorm.qemu-only.esp/EFI/BOOT
    cp "$EFI" /work/buildstorm.qemu-only.esp/EFI/BOOT/BOOTLOONGARCH64.EFI
    cp "$QEMU_VARS" /work/buildstorm.qemu-only.vars.fd
    timeout 60 /usr/bin/stdbuf -o0 -e0 \
        "$QEMU_LD" --library-path "$QROOT/lib" "$QEMU_BIN" \
        -L "$QROOT/share/qemu" \
        -machine virt -cpu la464 -smp 1 -m 2G -nographic -serial mon:stdio \
        -drive if=pflash,format=raw,unit=0,readonly=on,file="$QEMU_CODE" \
        -drive if=pflash,format=raw,unit=1,file=/work/buildstorm.qemu-only.vars.fd \
        -drive format=raw,file=fat:rw:/work/buildstorm.qemu-only.esp \
        2>&1 | tee "$RUN_OUT"
elif [ "$BYTES" -ge 500000 ]; then
    QEMU_LD=/opt/qemu-rv64/lib/ld-linux-riscv64-lp64d.so.1
    QEMU_BIN=/opt/qemu-rv64/bin/qemu-system-riscv64
    QEMU_BIOS=/opt/qemu-rv64/share/opensbi-riscv64-generic-fw_dynamic.bin
    timeout 30 /usr/bin/stdbuf -o0 -e0 \
        "$QEMU_LD" --library-path "$(dirname "$QEMU_LD")" "$QEMU_BIN" \
        -machine virt -smp 1 -m 256M -nographic \
        -bios "$QEMU_BIOS" -kernel "$ART" \
        2>&1 | tee "$RUN_OUT"
fi

if grep -qi 'hello, world' "$RUN_OUT" 2>/dev/null; then
    echo "QEMU_ONLY_RESULT arch=$ARCH status=OK run=OK"
else
    echo "QEMU_ONLY_RESULT arch=$ARCH status=FAIL run=FAIL"
fi

echo "#### OS COMP TEST GROUP END buildstorm-qemu-only ####"
sync
