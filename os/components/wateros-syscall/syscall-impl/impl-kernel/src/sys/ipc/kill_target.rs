#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillTargetSelector {
    Process(usize),
    CurrentProcessGroup,
    Broadcast,
    ProcessGroup(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SignalIdentity {
    pub(crate) real_uid : u32,
    pub(crate) effective_uid : u32,
    pub(crate) saved_uid : u32,
    pub(crate) session_id : usize,
}

pub(crate) fn can_signal(caller : SignalIdentity,
                         target : SignalIdentity,
                         signal : usize,
                         privileged : bool)
                         -> bool {
    const SIGCONT : usize = 18;

    privileged ||
    caller.real_uid == target.real_uid ||
    caller.real_uid == target.saved_uid ||
    caller.effective_uid == target.real_uid ||
    caller.effective_uid == target.saved_uid ||
    (signal == SIGCONT && caller.session_id != 0 && caller.session_id == target.session_id)
}

pub(crate) fn classify_kill_target(pid : isize) -> KillTargetSelector {
    match pid {
        p if p > 0 => KillTargetSelector::Process(p as usize),
        0 => KillTargetSelector::CurrentProcessGroup,
        -1 => KillTargetSelector::Broadcast,
        p => KillTargetSelector::ProcessGroup(p.unsigned_abs()),
    }
}

#[cfg(test)]
mod tests {
    use super::{can_signal, classify_kill_target, KillTargetSelector, SignalIdentity};

    fn identity(real_uid : u32,
                effective_uid : u32,
                saved_uid : u32,
                session_id : usize)
                -> SignalIdentity {
        SignalIdentity { real_uid,
                         effective_uid,
                         saved_uid,
                         session_id }
    }

    #[test]
    fn classifies_linux_kill_pid_forms() {
        assert_eq!(classify_kill_target(42),
                   KillTargetSelector::Process(42));
        assert_eq!(classify_kill_target(0),
                   KillTargetSelector::CurrentProcessGroup);
        assert_eq!(classify_kill_target(-1),
                   KillTargetSelector::Broadcast);
        assert_eq!(classify_kill_target(-42),
                   KillTargetSelector::ProcessGroup(42));
        assert_eq!(classify_kill_target(isize::MIN),
                   KillTargetSelector::ProcessGroup(isize::MIN.unsigned_abs()));
    }

    #[test]
    fn enforces_linux_signal_uid_and_session_rules() {
        let caller = identity(1000, 1001, 1002, 7);

        assert!(can_signal(caller,
                           identity(1000, 2000, 2001, 8),
                           15,
                           false));
        assert!(can_signal(caller,
                           identity(1001, 2000, 2001, 8),
                           15,
                           false));
        assert!(can_signal(caller,
                           identity(2000, 2001, 1001, 8),
                           15,
                           false));
        assert!(!can_signal(caller,
                            identity(1002, 2001, 2002, 8),
                            15,
                            false));
        assert!(!can_signal(caller,
                            identity(2000, 2001, 2002, 8),
                            15,
                            false));
        assert!(can_signal(caller,
                           identity(2000, 2001, 2002, 8),
                           15,
                           true));
        assert!(can_signal(caller,
                           identity(2000, 2001, 2002, 7),
                           18,
                           false));
        assert!(!can_signal(identity(1000, 1001, 1002, 0),
                            identity(2000, 2001, 2002, 0),
                            18,
                            false));
    }
}
