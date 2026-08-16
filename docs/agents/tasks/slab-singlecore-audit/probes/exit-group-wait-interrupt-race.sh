#!/bin/sh

echo "#### OS COMP TEST GROUP START buildstorm-glibc ####"
i=0
while [ "$i" -lt 500 ]; do
    timeout 10 /glibc/exit_group_race
    rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "EXIT_GROUP_RACE_RESULT status=FAIL iteration=$i rc=$rc"
        echo "#### OS COMP TEST GROUP END buildstorm-glibc ####"
        exit 1
    fi
    i=$((i + 1))
done
echo "EXIT_GROUP_RACE_RESULT status=OK iterations=$i"
echo "#### OS COMP TEST GROUP END buildstorm-glibc ####"
