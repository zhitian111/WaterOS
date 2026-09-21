# WaterOS VisionFive 2 U-Boot script (build with: mkimage -T script).
# 出厂 U-Boot（SDK Release 31）默认 bootpart=3 / rootpart=4；本脚本按官方
# 分区编号从 mmc 1:3 加载，rootfs 在 P4。DTB 缺省回退到 $fdtcontroladdr。
setenv bootargs 'root=/dev/mmcblk1p4 rw console=ttyS0,115200n8'
load mmc 1:3 ${kernel_addr_r} wateros-jh7110.ui
if load mmc 1:3 ${fdt_addr_r} jh7110-starfive-visionfive-2-v1.3b.dtb; then
    bootm ${kernel_addr_r} - ${fdt_addr_r}
else
    bootm ${kernel_addr_r} - ${fdtcontroladdr}
fi
