#!/glibc/busybox sh

cd /glibc
./busybox echo "#### FILE SEQUENCE DEBUG START ####"
./fstime -w -t 2 -b 256 -m 500 >/dev/null
./fstime -r -t 2 -b 256 -m 500 >/dev/null
./fstime -c -t 2 -b 256 -m 500 >/dev/null
./fstime -w -t 2 -b 1024 -m 2000 >/dev/null
./fstime -r -t 2 -b 1024 -m 2000 >/dev/null
./fstime -c -t 2 -b 1024 -m 2000 >/dev/null
./busybox echo "#### FILE SEQUENCE PRIMED ####"
./fstime -w -t 3 -b 4096 -m 8000 | ./busybox grep -o "WRITE COUNT|[[:digit:]]\+|" | ./busybox grep -o "[[:digit:]]\+" | ./busybox awk '{print "Unixbench FS_WRITE_BIG test(KBps): "$0}'
./busybox echo "#### FILE SEQUENCE DEBUG END ####"
