use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Pure software anti-chatter (keyboard) and anti-double-click (mouse) state machine.
///
/// Tracks the last valid release instant per key/button AND records suppressed bounce `DOWN`
/// events so their matching bounce `UP` is also swallowed without resetting the release window.
#[derive(Debug, Default)]
pub struct ChatterFilter {
    last_key_up: HashMap<u16, Instant>,
    suppressed_key_down: HashSet<u16>,
    last_mouse_up: [Option<Instant>; 2],
    suppressed_mouse_down: [bool; 2],
}

impl ChatterFilter {
    pub fn clear(&mut self) {
        self.last_key_up.clear();
        self.suppressed_key_down.clear();
        self.last_mouse_up = [None; 2];
        self.suppressed_mouse_down = [false; 2];
    }

    /// Evaluate a keyboard transition. Returns `true` when the event must be swallowed.
    pub fn check_key(&mut self, vk: u16, is_down: bool, window_ms: u16, now: Instant) -> bool {
        if window_ms == 0 {
            if !is_down {
                self.suppressed_key_down.remove(&vk);
            }
            return false;
        }

        if is_down {
            if let Some(prev_up) = self.last_key_up.get(&vk)
                && now.saturating_duration_since(*prev_up).as_millis() < window_ms as u128
            {
                self.suppressed_key_down.insert(vk);
                return true;
            }
            false
        } else if self.suppressed_key_down.remove(&vk) {
            true
        } else {
            self.last_key_up.insert(vk, now);
            false
        }
    }

    /// Evaluate a mouse button transition (`btn_idx` `0` = LMB, `1` = RMB).
    /// Returns `true` when the event must be swallowed.
    pub fn check_mouse(
        &mut self,
        btn_idx: usize,
        is_down: bool,
        window_ms: u16,
        now: Instant,
    ) -> bool {
        let Some(last_up_slot) = self.last_mouse_up.get_mut(btn_idx) else {
            return false;
        };
        let supp_slot = &mut self.suppressed_mouse_down[btn_idx];

        if window_ms == 0 {
            if !is_down {
                *supp_slot = false;
            }
            return false;
        }

        if is_down {
            if let Some(prev_up) = *last_up_slot
                && now.saturating_duration_since(prev_up).as_millis() < window_ms as u128
            {
                *supp_slot = true;
                return true;
            }
            false
        } else if *supp_slot {
            *supp_slot = false;
            true
        } else {
            *last_up_slot = Some(now);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bounce_down_and_paired_up_are_both_swallowed_without_extending_window() {
        let mut f = ChatterFilter::default();
        let t0 = Instant::now();

        // Legitimate press + release at t0 + 10ms
        assert!(!f.check_key(0x41, true, 20, t0));
        assert!(!f.check_key(0x41, false, 20, t0 + Duration::from_millis(10)));

        // Mechanical chatter bounce 5ms later: DOWN swallowed, paired UP also swallowed
        assert!(f.check_key(0x41, true, 20, t0 + Duration::from_millis(15)));
        assert!(f.check_key(0x41, false, 20, t0 + Duration::from_millis(17)));

        // Legitimate press 21ms after genuine release (t0+10+21 = t0+31) succeeds even though
        // only 14ms elapsed since the bounce UP!
        assert!(!f.check_key(0x41, true, 20, t0 + Duration::from_millis(31)));
    }

    #[test]
    fn mouse_double_click_suppresses_paired_release() {
        let mut f = ChatterFilter::default();
        let t0 = Instant::now();

        assert!(!f.check_mouse(0, false, 30, t0));
        assert!(f.check_mouse(0, true, 30, t0 + Duration::from_millis(10)));
        assert!(f.check_mouse(0, false, 30, t0 + Duration::from_millis(12)));
        assert!(!f.check_mouse(0, true, 30, t0 + Duration::from_millis(31)));
    }
}
