# LoongArch COW fault page-table walk

Source log: `../logs/wateros-la-fault-probe-full-3.log`.

The retained serial probe captured:

```text
proc pid=156 tid=1214 task_id=1410 role=Member
fault cause=Exception(StorePageFault) raw=0x40000 ecode=0x4
sepc=0x101d2afc stval=0x70040b40 sp=0x313d6630 tp=0x313d7620
satp=0x700096b2b2000 aspace=0x96232cf0
```

The stopped-VM page-table walk used token PGDL `0x96b2b2000`, ASID 7, and
virtual address `0x70040b40`. The three VPN indices were `[64, 384, 1]`:

```text
root entry @ 0x96b2b2008 = 0x96a605000
L1 entry   @ 0x96a605c00 = 0x966970000
leaf entry @ 0x966970200 = 0x40000009621ea19f
```

The leaf decoded as valid/present/writable/dirty/user (PLV3), with NX set.
Therefore the architectural PTE was already writable and dirty when the CPU
reported PME (`ecode=4`). This is strong evidence for a stale D=0 translation:
another CPU completed the same COW transition before this CPU acquired the
address-space lock. Before commit `705950b4`, the second CPU then saw a
non-COW PTE, returned `false`, and the trap path killed the user task. Commit
`705950b4` treats an already writable+dirty+user level-0 leaf as a handled COW
race so the outer wrapper invalidates translations and retries.

This analysis establishes the mechanism of the observed SIGSEGV. It does not
establish that the separate hashbrown/UEFI stalls are fixed.
