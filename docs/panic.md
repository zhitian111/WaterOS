+--------------- WaterOS CPU Dashboard (tick=2641) ------------------+
|                                                                         |
+------+----------------+--------+-----------+------+--------+----------+
| CPU  | Current Task   | State  | Q O/F/R   | Rsch | Switch | Timer    |
+------+----------------+--------+-----------+------+--------+----------+
|    0 |             10 | KERN   |     1/0/0 | YES  |   5206 |     2641 |
|    1 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    2 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    3 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    4 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    5 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    6 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
|    7 |              - | OFF    |     0/0/0 | -    |      0 |        0 |
+------+----------------+--------+-----------+------+--------+----------+
/musl/ltp_testcode.sh: line 15: ltp/testcases/bin/close_range02: not found
FAIL LTP CASE close_range02 : 127
RUN LTP CASE confstr01
/musl/ltp_testcode.sh: line 15: ltp/testcases/bin/confstr01: not found
FAIL LTP CASE confstr01 : 127
RUN LTP CASE connect01
/musl/ltp_testcode.sh: line 15: ltp/testcases/bin/connect01: not found
FAIL LTP CASE connect01 : 127
RUN LTP CASE crash01
crash01     0  TINFO  :  crashme +2000.80 34 100
crash01     1  TPASS  :  we're still here, OS seems to be robust
exit status ... number of cases
          0 ...     1
FAIL LTP CASE crash01 : 0
RUN LTP CASE crash02
crash02     0  TINFO  :  crashme02 127 34 100
0000: syscall(19, 0x50879292, 0x27, 0xd9dbdc06, 0x52df8d3e, 0x8f3cf180, 0, 0xbb)
0001: syscall(11, 0x91d00589, 0, 0, 0x518e40af, 0, 0xeef61444, 0)
0002: syscall(104, 0xb02bd967, 0x8431f823, 0x93f5c1d0, 0, 0, 0x84a41d4a, 0x5ff2f51d)
0003: syscall(28, 0x3, 0xf2046f3e, 0x221422e6, 0xf7, 0xdd926e51, 0, 0xd02b7b02)
0004: syscall(116, 0x423e3dc9, 0, 0x11e12a0, 0, 0, 0x7e38382f, 0xadf73d56)
0005: syscall(114, 0xc508d1ad, 0x8a357fc4, 0x4ac3bc95, 0x6a, 0, 0, 0xa9f3bffd)
0006: syscall(116, 0x2af, 0xceccfdb2, 0, 0x27ac77e1, 0x7e88214e, 0, 0xaaa3996c)
[WaterOS]    [PANIC] Panicked at components/wateros-klog/klog-impl/klog-ringbuf/src/lib.rs:174.  index out of bounds: the len is 256 but the index is 576460752303423488
