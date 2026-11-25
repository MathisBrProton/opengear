use std::collections::HashMap;

/// SOCD resolution strategy for an opposing key pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SocdMode {
    /// Most recent physical press wins (Snap Tap / FlashTap behavior).
    #[default]
    LastInputWins,
    /// Holding both keys simultaneously cancels both outputs (neutral).
    Neutral,
    /// First key in the pair always wins while both are held.
    PriorityFirst,
    /// Second key in the pair always wins while both are held.
    PrioritySecond,
}

/// Physical key identity paired with its virtual-key fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocdKey {
    pub vk: u16,
    pub scan: u16,
    pub extended: bool,
}

/// Compiled opposing key pair used by [`SocdResolver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocdPair {
    pub a: SocdKey,
    pub b: SocdKey,
    pub mode: SocdMode,
}

impl SocdPair {
    /// Returns `Some(true)` for key A, `Some(false)` for key B, or `None` if unrelated.
    /// Physical scan code takes precedence over the virtual-key fallback.
    pub fn side_of(&self, vk: u16, scan: u16) -> Option<bool> {
        if scan != 0 {
            if scan == self.a.scan {
                return Some(true);
            }
            if scan == self.b.scan {
                return Some(false);
            }
        }
        if vk == self.a.vk {
            Some(true)
        } else if vk == self.b.vk {
            Some(false)
        } else {
            None
        }
    }

    #[inline]
    pub fn key(&self, is_a: bool) -> SocdKey {
        if is_a { self.a } else { self.b }
    }
}

/// Deterministic action emitted by [`SocdResolver::handle_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocdDecision {
    /// Whether the current physical event should be passed downstream (`CallNextHookEx`).
    pub pass_current: bool,
    /// Optional opposing event to inject BEFORE passing the current event downstream.
    pub pre_inject: Option<(SocdKey, bool)>,
    /// Optional opposing event to inject AFTER passing the current event downstream.
    pub post_inject: Option<(SocdKey, bool)>,
    /// True when SOCD actively suppressed or neutralized a conflict.
    pub resolved_conflict: bool,
}

impl SocdDecision {
    const fn pass() -> Self {
        Self {
            pass_current: true,
            pre_inject: None,
            post_inject: None,
            resolved_conflict: false,
        }
    }

    const fn suppress() -> Self {
        Self {
            pass_current: false,
            pre_inject: None,
            post_inject: None,
            resolved_conflict: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct PairState {
    a_held: bool,
    b_held: bool,
    active_is_a: Option<bool>,
}

/// Pure, allocation-minimal SOCD state machine shared across all configured pairs.
#[derive(Debug, Default)]
pub struct SocdResolver {
    states: HashMap<(u16, u16), PairState>,
}

impl SocdResolver {
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// Update state for one physical key transition and return exact pass/inject actions.
    pub fn handle_event(&mut self, pair: &SocdPair, is_a: bool, is_down: bool) -> SocdDecision {
        let key = (pair.a.vk, pair.b.vk);
        let st = self.states.entry(key).or_default();
        let opp_key = pair.key(!is_a);

        if is_down {
            let was_held = if is_a { st.a_held } else { st.b_held };
            if is_a {
                st.a_held = true;
            } else {
                st.b_held = true;
            }
            let opp_held = if is_a { st.b_held } else { st.a_held };

            if was_held {
                return if st.active_is_a == Some(is_a) {
                    SocdDecision::pass()
                } else {
                    SocdDecision::suppress()
                };
            }

            match pair.mode {
                SocdMode::LastInputWins => {
                    let prev_active_opp = st.active_is_a == Some(!is_a);
                    st.active_is_a = Some(is_a);
                    if prev_active_opp {
                        SocdDecision {
                            pass_current: true,
                            pre_inject: Some((opp_key, false)),
                            post_inject: None,
                            resolved_conflict: true,
                        }
                    } else {
                        SocdDecision::pass()
                    }
                }
                SocdMode::Neutral => {
                    if opp_held {
                        let prev_active_opp = st.active_is_a == Some(!is_a);
                        st.active_is_a = None;
                        SocdDecision {
                            pass_current: false,
                            pre_inject: prev_active_opp.then_some((opp_key, false)),
                            post_inject: None,
                            resolved_conflict: true,
                        }
                    } else {
                        st.active_is_a = Some(is_a);
                        SocdDecision::pass()
                    }
                }
                SocdMode::PriorityFirst | SocdMode::PrioritySecond => {
                    let winner_is_a = matches!(pair.mode, SocdMode::PriorityFirst);
                    if opp_held {
                        if is_a == winner_is_a {
                            let prev_active_opp = st.active_is_a == Some(!is_a);
                            st.active_is_a = Some(is_a);
                            SocdDecision {
                                pass_current: true,
                                pre_inject: prev_active_opp.then_some((opp_key, false)),
                                post_inject: None,
                                resolved_conflict: true,
                            }
                        } else {
                            SocdDecision {
                                pass_current: false,
                                pre_inject: None,
                                post_inject: None,
                                resolved_conflict: true,
                            }
                        }
                    } else {
                        st.active_is_a = Some(is_a);
                        SocdDecision::pass()
                    }
                }
            }
        } else {
            if is_a {
                st.a_held = false;
            } else {
                st.b_held = false;
            }
            let opp_held = if is_a { st.b_held } else { st.a_held };

            match pair.mode {
                SocdMode::Neutral => {
                    if opp_held && st.active_is_a.is_none() {
                        st.active_is_a = Some(!is_a);
                        SocdDecision {
                            pass_current: true,
                            pre_inject: None,
                            post_inject: Some((opp_key, true)),
                            resolved_conflict: true,
                        }
                    } else if st.active_is_a == Some(is_a) {
                        st.active_is_a = None;
                        SocdDecision::pass()
                    } else {
                        SocdDecision::pass()
                    }
                }
                SocdMode::LastInputWins | SocdMode::PriorityFirst | SocdMode::PrioritySecond => {
                    if st.active_is_a == Some(is_a) {
                        if opp_held {
                            st.active_is_a = Some(!is_a);
                            SocdDecision {
                                pass_current: true,
                                pre_inject: None,
                                post_inject: Some((opp_key, true)),
                                resolved_conflict: false,
                            }
                        } else {
                            st.active_is_a = None;
                            SocdDecision::pass()
                        }
                    } else {
                        SocdDecision::pass()
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasd_pair(mode: SocdMode) -> SocdPair {
        SocdPair {
            a: SocdKey { vk: 0x51, scan: 0x1E, extended: false },
            b: SocdKey { vk: 0x44, scan: 0x20, extended: false },
            mode,
        }
    }

    fn arrows_pair(mode: SocdMode) -> SocdPair {
        SocdPair {
            a: SocdKey { vk: 0x25, scan: 0x4B, extended: true },
            b: SocdKey { vk: 0x27, scan: 0x4D, extended: true },
            mode,
        }
    }

    #[test]
    fn last_input_wins_pre_releases_and_reengages_extended_keys() {
        let mut r = SocdResolver::default();
        let p = arrows_pair(SocdMode::LastInputWins);

        // Press Left -> passes cleanly
        let d1 = r.handle_event(&p, true, true);
        assert!(d1.pass_current && d1.pre_inject.is_none());

        // Press Right while Left held -> releases Left (with extended=true!) before Right down
        let d2 = r.handle_event(&p, false, true);
        assert!(d2.pass_current);
        assert!(d2.resolved_conflict);
        assert_eq!(d2.pre_inject, Some((p.a, false)));

        // Auto-repeat on suppressed Left -> suppressed
        let d_rep = r.handle_event(&p, true, true);
        assert!(!d_rep.pass_current);

        // Release Right while Left still held -> Right UP passes, Left re-pressed afterwards
        let d3 = r.handle_event(&p, false, false);
        assert!(d3.pass_current);
        assert_eq!(d3.post_inject, Some((p.a, true)));
    }

    #[test]
    fn neutral_cancels_both_then_restores_remaining_key() {
        let mut r = SocdResolver::default();
        let p = wasd_pair(SocdMode::Neutral);

        assert!(r.handle_event(&p, true, true).pass_current);
        let conflict = r.handle_event(&p, false, true);
        assert!(!conflict.pass_current);
        assert_eq!(conflict.pre_inject, Some((p.a, false)));
        assert!(conflict.resolved_conflict);

        let release_a = r.handle_event(&p, true, false);
        assert!(release_a.pass_current);
        assert_eq!(release_a.post_inject, Some((p.b, true)));
    }

    #[test]
    fn priority_first_suppresses_second_even_if_pressed_later() {
        let mut r = SocdResolver::default();
        let p = wasd_pair(SocdMode::PriorityFirst);

        assert!(r.handle_event(&p, true, true).pass_current);
        let b_down = r.handle_event(&p, false, true);
        assert!(!b_down.pass_current);
        assert!(b_down.resolved_conflict);

        let a_up = r.handle_event(&p, true, false);
        assert_eq!(a_up.post_inject, Some((p.b, true)));
    }
}
