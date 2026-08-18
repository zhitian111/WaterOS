#!/bin/sh
# 定向检查 cgroup 兼容树不会虚报尚未实现的控制器。

echo "#### OS COMP TEST GROUP START cgroup-capability ####"

target_dir="ltp/testcases/bin"
failed=0

for case_name in cgroup_core01 cgroup_core02 cgroup_core03; do
    case_path="$target_dir/$case_name"
    echo "RUN LTP CASE $case_name"
    "$case_path"
    ret=$?
    echo "RESULT LTP CASE $case_name : $ret"

    # LTP 的 32 是 TCONF：内核没有声明所需控制器，属于本回归的预期结果。
    if [ "$ret" -ne 32 ]; then
        failed=1
    fi
done

echo "#### OS COMP TEST GROUP END cgroup-capability ####"
exit "$failed"
