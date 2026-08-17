# 内存/地址空间细节（可能暴露 brk/mmap 更多问题）

stress-ng --vm-addr 2 --vma 2 --mmap 2 --mmapfork 2 --timeout=20 --metrics

# 文件系统（open/read/write/stat 风暴）

stress-ng --open 4 --fstat 4 --dir 2 --seek 2
  --chmod 2 --chown 2 --rename 2 --symlink 2 --getdent 2
  --timeout=20 --metrics

# 调度/进程

stress-ng --forkheavy 2 --pipeherd 2 --context 4 --timeout=20 --metrics

# 全类（看哪些 stressor 报错）

stress-ng --all --timeout=30 --metrics 2>&1 | grep -E "fail|error|skipped"

[WaterOS][cpu=3]    [WARN]  [syscall] renameat2(nr=276) unsupported flags=0xffffffff
stress-ng: fail:  [15] rename: renameat2 unexpectedly succeeded on existent directory/file with RENAME_NOREPLACE flag, errno=2 (No such file or directory)
[WaterOS][cpu=3]    [WARN]  [syscall] renameat2(nr=276) unsupported flags=0xffffffff
stress-ng: fail:  [15] rename: renameat2 unexpectedly succeeded on existent directory/file with RENAME_NOREPLACE flag, errno=2 (No such file or directory)
[WaterOS][cpu=3]    [WARN]  [syscall] renameat2(nr=276) unsupported flags=0xffffffff
stress-ng: fail:  [15] rename: renameat2 unexpectedly succeeded on existent directory/file with RENAME_NOREPLACE flag, errno=2 (No such file or directory)
[WaterOS][cpu=3]    [WARN]  [syscall] renameat2(nr=276) unsupported flags=0xffffffff
stress-ng: fail:  [15] rename: renameat2 unexpectedly succeeded on existent directory/file with RENAME_NOREPLACE flag, errno=2 (No such file or directory)
[WaterOS][cpu=3]    [WARN]  [syscall] renameat2(nr=276) unsupported flags=0xffffffff
stress-ng: fail:  [15] rename: renameat2 unexpectedly succeeded on existent directory/file with RENAME_
