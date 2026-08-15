#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GetGroupsPlan {
    Query(usize),
    Copy(usize),
}

pub(crate) fn plan_getgroups(raw_size : usize, group_count : usize) -> Option<GetGroupsPlan> {
    if (raw_size as isize) < 0 {
        return None;
    }
    if raw_size == 0 {
        return Some(GetGroupsPlan::Query(group_count));
    }
    if raw_size < group_count {
        return None;
    }
    Some(GetGroupsPlan::Copy(group_count))
}

// LTP setgroups03 后改为 NGROUPS_MAX+先拷贝的校验顺序；保留供参考/测试。
#[allow(dead_code)]
pub(crate) fn valid_setgroups_size(size : usize, maximum : usize) -> bool { size <= maximum }

#[cfg(test)]
mod tests {
    use super::{plan_getgroups, valid_setgroups_size, GetGroupsPlan};

    #[test]
    fn getgroups_distinguishes_query_and_copy_modes() {
        assert_eq!(plan_getgroups(0, 3),
                   Some(GetGroupsPlan::Query(3)));
        assert_eq!(plan_getgroups(3, 3),
                   Some(GetGroupsPlan::Copy(3)));
        assert_eq!(plan_getgroups(8, 3),
                   Some(GetGroupsPlan::Copy(3)));
    }

    #[test]
    fn getgroups_rejects_negative_or_short_lengths() {
        assert_eq!(plan_getgroups((-1isize) as usize, 3),
                   None);
        assert_eq!(plan_getgroups(2, 3), None);
    }

    #[test]
    fn setgroups_rejects_only_lengths_above_the_limit() {
        assert!(valid_setgroups_size(0, 32));
        assert!(valid_setgroups_size(32, 32));
        assert!(!valid_setgroups_size(33, 32));
        assert!(!valid_setgroups_size(usize::MAX, 32));
    }
}
