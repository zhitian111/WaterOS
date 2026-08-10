# LS2K1000 MMC 只读证据采集

此流程用于两块目标板到位后的第一轮 MMC 只读检查。它不会执行 MMC command，也不能解除
CRC/busy policy gate。远程 monitor 没有认证和加密，只能接入隔离开发网络。

## 准备

1. 复制 `docs/tasks/ls2k-mmc-evidence-manifest-v2.json` 到一个空采集目录。
2. 用实际资产编号替换 `ls2k1000-board-a` 和 `ls2k1000-board-b`；编号只能包含 ASCII
   字母、数字、点、下划线和短横线。
3. 若板卡为 non-removable，先评审并调整模板中的 `card`/`present` 断言，不得为了通过校验
   临时删除失败字段。
4. 使用带 `remote-debug-monitor` 的内核，并确保没有其他 MMC controller owner。

## 采集矩阵

每块板的每个场景至少采集两次，文件不得复用，采集时间必须不同：

- `cold-no-card`：断电，移除卡片，重新上电后采集。
- `cold-card`：断电，插入卡片，重新上电后采集。
- `warm-card`：保持卡片插入，执行一次受控热重启后采集。

每次执行：

```bash
python3 os/scripts/remote_debug_client.py \
  --host BOARD_IP --port 22323 \
  --board-id BOARD_ID \
  --mmc-evidence BOARD_ID-SCENARIO-SAMPLE.json
```

随后在 manifest 的 `evidence` 数组中追加：

```json
{"board_id":"BOARD_ID","scenario":"SCENARIO","path":"BOARD_ID-SCENARIO-SAMPLE.json"}
```

## 离线验收

每次采集后先校验单文件：

```bash
python3 os/scripts/mmc_evidence_verify.py --evidence PATH.json
```

全部采集后校验矩阵：

```bash
python3 os/scripts/mmc_evidence_verify.py --manifest ls2k-mmc-evidence-manifest-v2.json
```

只有输出 `"complete":true` 且退出码为 0 时，软件归档矩阵才完整。此结果仍只是
`unverified-observation`：必须结合串口日志、板卡身份和人工执行记录评审，不能视为存储功能可用。
