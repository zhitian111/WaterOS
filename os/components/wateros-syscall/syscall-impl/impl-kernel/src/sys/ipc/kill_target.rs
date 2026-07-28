#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillTargetSelector {
    Process(usize),
    CurrentProcessGroup,
    Broadcast,
    ProcessGroup(usize),
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
    use super::{classify_kill_target, KillTargetSelector};

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
}
