CPU A（新任务入队）                     CPU B（当前运行）
      |                                      |
      |-- enqueue_woken_task()                |
      |   request_reschedule(B)               |
      |     need_resched[B] = true            |
      |     send_ipi(B) --------------------> SupervisiorSoft Interrupt
      |                                      |
      |                            trap_handler.rs:270
      |                            task::schedule_reschedule()
      |                              take_need_resched(B) == true
      |                              scheduler.schedule(Reschedule, B)
      |                              __switch() → 切换任务
      |                                      |
      |                            sret（返回用户态/新任务）
