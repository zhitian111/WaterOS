#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IdTriplet {
    pub(crate) real : u32,
    pub(crate) effective : u32,
    pub(crate) saved : u32,
}

fn is_current_id(current : IdTriplet, requested : u32) -> bool {
    requested == current.real || requested == current.effective || requested == current.saved
}

pub(crate) fn plan_set_id(current : IdTriplet,
                          requested : u32,
                          privileged : bool)
                          -> Option<IdTriplet> {
    if privileged {
        return Some(IdTriplet { real : requested,
                                effective : requested,
                                saved : requested });
    }
    if requested != current.real && requested != current.saved {
        return None;
    }
    Some(IdTriplet { effective : requested,
                     ..current })
}

pub(crate) fn plan_set_re_id(current : IdTriplet,
                             requested_real : Option<u32>,
                             requested_effective : Option<u32>,
                             privileged : bool)
                             -> Option<IdTriplet> {
    if !privileged {
        if requested_real.is_some_and(|id| id != current.real && id != current.effective) {
            return None;
        }
        if requested_effective.is_some_and(|id| !is_current_id(current, id)) {
            return None;
        }
    }

    let mut next = current;
    if let Some(id) = requested_real {
        next.real = id;
    }
    if let Some(id) = requested_effective {
        next.effective = id;
    }
    if requested_real.is_some() || requested_effective.is_some_and(|id| id != current.real) {
        next.saved = next.effective;
    }
    Some(next)
}

pub(crate) fn plan_set_res_id(current : IdTriplet,
                              requested_real : Option<u32>,
                              requested_effective : Option<u32>,
                              requested_saved : Option<u32>,
                              privileged : bool)
                              -> Option<IdTriplet> {
    if !privileged &&
       [requested_real,
        requested_effective,
        requested_saved].into_iter()
                        .flatten()
                        .any(|id| !is_current_id(current, id))
    {
        return None;
    }

    Some(IdTriplet { real : requested_real.unwrap_or(current.real),
                     effective : requested_effective.unwrap_or(current.effective),
                     saved : requested_saved.unwrap_or(current.saved) })
}

#[cfg(test)]
mod tests {
    use super::{plan_set_id, plan_set_re_id, plan_set_res_id, IdTriplet};

    const USER : IdTriplet = IdTriplet { real : 100,
                                         effective : 200,
                                         saved : 300 };

    #[test]
    fn privileged_set_id_replaces_all_ids() {
        assert_eq!(plan_set_id(USER, 42, true),
                   Some(IdTriplet { real : 42,
                                    effective : 42,
                                    saved : 42 }));
    }

    #[test]
    fn unprivileged_set_id_only_switches_effective_id() {
        assert_eq!(plan_set_id(USER, 100, false),
                   Some(IdTriplet { real : 100,
                                    effective : 100,
                                    saved : 300 }));
        assert_eq!(plan_set_id(USER, 300, false),
                   Some(IdTriplet { real : 100,
                                    effective : 300,
                                    saved : 300 }));
        assert_eq!(plan_set_id(USER, 400, false), None);
    }

    #[test]
    fn set_re_id_checks_targets_and_updates_saved_id() {
        assert_eq!(plan_set_re_id(USER, Some(200), None, false),
                   Some(IdTriplet { real : 200,
                                    effective : 200,
                                    saved : 200 }));
        assert_eq!(plan_set_re_id(USER, None, Some(100), false),
                   Some(IdTriplet { real : 100,
                                    effective : 100,
                                    saved : 300 }));
        assert_eq!(plan_set_re_id(USER, Some(300), None, false),
                   None);
        assert_eq!(plan_set_re_id(USER, None, Some(400), false),
                   None);
    }

    #[test]
    fn set_res_id_limits_each_non_root_target_to_current_ids() {
        assert_eq!(plan_set_res_id(USER,
                                   Some(200),
                                   Some(300),
                                   Some(100),
                                   false),
                   Some(IdTriplet { real : 200,
                                    effective : 300,
                                    saved : 100 }));
        assert_eq!(plan_set_res_id(USER, None, Some(400), None, false),
                   None);
        assert_eq!(plan_set_res_id(USER,
                                   Some(400),
                                   Some(500),
                                   Some(600),
                                   true),
                   Some(IdTriplet { real : 400,
                                    effective : 500,
                                    saved : 600 }));
    }
}
