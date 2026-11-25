use crate::chatter::ChatterFilter;
use crate::keys::{key_name_to_vk, vk_to_key_name};
use crate::socd::{SocdDecision, SocdKey, SocdMode, SocdPair, SocdResolver};
use opengear_core::{KeyAction, MacroStep, SocdConfig};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{error, info};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MapVirtualKeyExW, SendInput,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const OPENGEAR_INJECTED_TAG: usize = 0x4F47_4541;
const LLMHF_INJECTED: u32 = 0x0000_0001;
#[cfg(windows)]
const WM_SYNC_MOUSE_HOOK: u32 = WM_APP + 77;
#[cfg(windows)]
const MAPVK_VK_TO_VSC_EX: u32 = 4;

/// Longest pause a single macro step may request (longer values are clamped).
const MAX_MACRO_STEP_DELAY_MS: u32 = 10_000;
/// Longest per-character pause for text actions.
const MAX_TEXT_CHAR_DELAY_MS: u32 = 1_000;
/// Granularity at which running macros check for cancellation while sleeping.
const MACRO_CANCEL_POLL: Duration = Duration::from_millis(20);
/// After a failed hook installation, `start()` won't retry before this delay (the UI may call it
/// every frame for Reactive Typing).
const START_RETRY_BACKOFF: Duration = Duration::from_secs(5);

/// Lock-free per-virtual-key bookkeeping, safe to use from inside the low-level hook
/// (which must never block): physical held state, active remaps, and suppressed keys.
pub(crate) struct KeyTracker {
    /// Physically held keys as seen by the hook (non-injected events only).
    held: [AtomicBool; 256],
    /// For a source key currently remapped: the target VK OpenGear pressed (0 = none).
    remapped_to: [AtomicU16; 256],
    /// Keys whose `DOWN` event was swallowed by `win_lock` or `KeyAction::Disable`.
    suppressed_down: [AtomicBool; 256],
    /// Last hardware scan code seen for each VK (bit 15 = extended key), 0 = never seen.
    scan: [AtomicU16; 256],
}

impl KeyTracker {
    pub(crate) const fn new() -> Self {
        Self {
            held: [const { AtomicBool::new(false) }; 256],
            remapped_to: [const { AtomicU16::new(0) }; 256],
            suppressed_down: [const { AtomicBool::new(false) }; 256],
            scan: [const { AtomicU16::new(0) }; 256],
        }
    }

    /// Remember the scan code a physical key event carried (`extended` = LLKHF_EXTENDED).
    pub(crate) fn note_scan(&self, vk: u16, scan_code: u32, extended: bool) {
        if let Some(i) = Self::slot(vk) {
            let packed = (scan_code as u16 & 0x7FFF) | if extended { 0x8000 } else { 0 };
            if scan_code != 0 {
                self.scan[i].store(packed, Ordering::Relaxed);
            }
        }
    }

    /// Scan code (without the extended bit) and extended flag last observed for `vk`.
    pub(crate) fn observed_scan(&self, vk: u16) -> Option<(u16, bool)> {
        let i = Self::slot(vk)?;
        let packed = self.scan[i].load(Ordering::Relaxed);
        (packed != 0).then_some((packed & 0x7FFF, packed & 0x8000 != 0))
    }

    fn slot(vk: u16) -> Option<usize> {
        (vk < 256).then_some(vk as usize)
    }

    /// Record a physical key event. Returns `true` when it is an OS auto-repeat
    /// (a key-down while the key is already held).
    pub(crate) fn note_physical(&self, vk: u16, down: bool) -> bool {
        let Some(i) = Self::slot(vk) else {
            return false;
        };
        if down {
            self.held[i].swap(true, Ordering::SeqCst)
        } else {
            self.held[i].store(false, Ordering::SeqCst);
            false
        }
    }

    /// Forget every held key and suppressed state (events are not observed while stopped).
    pub(crate) fn clear_held(&self) {
        for h in &self.held {
            h.store(false, Ordering::SeqCst);
        }
        for s in &self.suppressed_down {
            s.store(false, Ordering::SeqCst);
        }
    }

    pub(crate) fn mark_suppressed(&self, vk: u16, suppressed: bool) {
        if let Some(i) = Self::slot(vk) {
            self.suppressed_down[i].store(suppressed, Ordering::SeqCst);
        }
    }

    pub(crate) fn take_suppressed(&self, vk: u16) -> bool {
        let Some(i) = Self::slot(vk) else {
            return false;
        };
        self.suppressed_down[i].swap(false, Ordering::SeqCst)
    }

    /// Remember that pressing `src` made OpenGear press `target`.
    pub(crate) fn begin_remap(&self, src: u16, target: u16) {
        if let Some(i) = Self::slot(src) {
            self.remapped_to[i].store(target, Ordering::SeqCst);
        }
    }

    /// Target currently held on behalf of `src`, if any.
    pub(crate) fn remap_target(&self, src: u16) -> Option<u16> {
        let i = Self::slot(src)?;
        Some(self.remapped_to[i].load(Ordering::SeqCst)).filter(|&t| t != 0)
    }

    /// `src` was released: return (and forget) the target that must be released with it.
    pub(crate) fn end_remap(&self, src: u16) -> Option<u16> {
        let i = Self::slot(src)?;
        Some(self.remapped_to[i].swap(0, Ordering::SeqCst)).filter(|&t| t != 0)
    }

    /// Take every target still held by an active remap (used when the engine stops).
    pub(crate) fn take_all_remaps(&self) -> Vec<u16> {
        self.remapped_to
            .iter()
            .map(|t| t.swap(0, Ordering::SeqCst))
            .filter(|&t| t != 0)
            .collect()
    }
}

/// Keep track of keys a macro has pressed but not yet released.
pub(crate) fn track_pressed(pressed: &mut Vec<u16>, vk: u16, down: bool) {
    if down {
        if !pressed.contains(&vk) {
            pressed.push(vk);
        }
    } else {
        pressed.retain(|&k| k != vk);
    }
}

pub(crate) fn capped_delay(ms: u32, max_ms: u32) -> Duration {
    Duration::from_millis(ms.min(max_ms) as u64)
}

static KEYS: KeyTracker = KeyTracker::new();
/// Bumped to cancel every running macro / text action (engine stop).
static MACRO_GENERATION: AtomicU32 = AtomicU32::new(0);

/// Sleep for `total`, waking early if macros of `generation` get cancelled.
/// Returns `false` when cancelled.
fn sleep_unless_cancelled(total: Duration, generation: u32) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if MACRO_GENERATION.load(Ordering::SeqCst) != generation {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(MACRO_CANCEL_POLL));
    }
}

/// True only for the standard WASD/ZQSD horizontal and vertical movement preset pairs.
/// Non-WASD custom pairs (e.g. `Q+E` or `Left+Right`) must NOT be overridden to movement scans.
pub(crate) fn is_movement_preset_pair(key_a: &str, key_b: &str) -> bool {
    let a = key_a.trim().to_ascii_uppercase();
    let b = key_b.trim().to_ascii_uppercase();
    matches!(
        (a.as_str(), b.as_str()),
        ("Q", "D")
            | ("D", "Q")
            | ("A", "D")
            | ("D", "A")
            | ("Z", "S")
            | ("S", "Z")
            | ("W", "S")
            | ("S", "W")
    )
}

/// Physical scan code for a Snap Tap movement preset key name (`Q`/`A`, `D`, `Z`/`W`, `S`).
pub(crate) fn movement_preset_scan(name: &str) -> Option<(u16, bool)> {
    match name.trim().to_ascii_uppercase().as_str() {
        "Q" | "A" => Some((0x1E, false)),
        "D" => Some((0x20, false)),
        "Z" | "W" => Some((0x11, false)),
        "S" => Some((0x1F, false)),
        _ => None,
    }
}

/// Resolve physical `(scan, extended)` for a Snap Tap pair endpoint.
pub(crate) fn resolve_pair_scan(name: &str, vk: u16, use_movement_presets: bool) -> (u16, bool) {
    if use_movement_presets && let Some(m) = movement_preset_scan(name) {
        return m;
    }
    #[cfg(windows)]
    {
        layout_scan_ex(vk)
    }
    #[cfg(not(windows))]
    {
        let _ = vk;
        (0, false)
    }
}

/// Decode the Windows `MAPVK_VK_TO_VSC_EX` return value into `(scan, extended)`.
pub(crate) fn decode_vsc_ex(raw: u32) -> (u16, bool) {
    let hi = (raw >> 8) & 0xFF;
    let lo = (raw & 0xFF) as u16;
    let extended = hi == 0xE0 || hi == 0xE1;
    (lo, extended)
}

/// Movement-cluster physical keys tracked for the live Snap Tap view: left, right, forward, back.
pub const MOVEMENT_SCANS: [u16; 4] = [0x1E, 0x20, 0x11, 0x1F];
static EFFECTIVE_DOWN: [AtomicBool; 4] = [const { AtomicBool::new(false) }; 4];

fn note_effective(scan: u32, down: bool) {
    if let Some(i) = MOVEMENT_SCANS.iter().position(|&s| s as u32 == scan) {
        EFFECTIVE_DOWN[i].store(down, Ordering::Relaxed);
    }
}

/// What applications currently receive for [`MOVEMENT_SCANS`] (left, right, forward, back).
pub fn movement_keys_effective() -> [bool; 4] {
    [0, 1, 2, 3].map(|i| EFFECTIVE_DOWN[i].load(Ordering::Relaxed))
}

#[derive(Debug, Default, Clone)]
struct RuntimeRules {
    win_lock: bool,
    by_vk: HashMap<u16, KeyAction>,
    socd_enabled: bool,
    socd_pairs: Vec<SocdPair>,
    key_chatter_ms: u16,
    mouse_debounce_ms: u16,
}

#[derive(Debug, Default)]
struct EngineState {
    socd: SocdResolver,
    chatter: ChatterFilter,
}

#[derive(Debug, Default)]
struct MacroRecorderState {
    recording: bool,
    last_event: Option<Instant>,
    steps: Vec<MacroStep>,
}

static GLOBAL_RULES: OnceLock<RwLock<RuntimeRules>> = OnceLock::new();
static RUNTIME_STATE: OnceLock<Mutex<EngineState>> = OnceLock::new();
static MACRO_RECORDER: OnceLock<Mutex<MacroRecorderState>> = OnceLock::new();
static REACTIVE_RIPPLES: OnceLock<Mutex<VecDeque<(&'static str, Instant)>>> = OnceLock::new();

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static SOCD_SUSPENDED: AtomicBool = AtomicBool::new(false);
static CHATTER_BLOCKED_COUNT: AtomicU32 = AtomicU32::new(0);
static SOCD_RESOLVED_COUNT: AtomicU32 = AtomicU32::new(0);
static INJECTED_OK_COUNT: AtomicU32 = AtomicU32::new(0);
static INJECTED_FAILED_COUNT: AtomicU32 = AtomicU32::new(0);
static MOUSE_DBLCLICK_BLOCKED_COUNT: AtomicU32 = AtomicU32::new(0);

fn rules_store() -> &'static RwLock<RuntimeRules> {
    GLOBAL_RULES.get_or_init(|| RwLock::new(RuntimeRules::default()))
}

fn state_store() -> &'static Mutex<EngineState> {
    RUNTIME_STATE.get_or_init(|| Mutex::new(EngineState::default()))
}

fn recorder_store() -> &'static Mutex<MacroRecorderState> {
    MACRO_RECORDER.get_or_init(|| Mutex::new(MacroRecorderState::default()))
}

fn ripples_store() -> &'static Mutex<VecDeque<(&'static str, Instant)>> {
    REACTIVE_RIPPLES.get_or_init(|| Mutex::new(VecDeque::with_capacity(32)))
}

#[cfg(windows)]
fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 // PgUp..Down
            | 0x2C  // PrtSc
            | 0x2D  // Ins
            | 0x2E  // Del
            | 0x5B  // LWin
            | 0x5C  // RWin
            | 0x5D  // Apps
            | 0xA3  // RCtrl
            | 0xA5  // RAlt
            | 0xAD..=0xB3 // Media
    )
}

#[cfg(windows)]
fn layout_scan_ex(vk: u16) -> (u16, bool) {
    unsafe {
        let fg = GetForegroundWindow();
        let tid = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        };
        let hkl = GetKeyboardLayout(tid);
        let raw = MapVirtualKeyExW(vk as u32, MAPVK_VK_TO_VSC_EX, hkl);
        decode_vsc_ex(raw)
    }
}

/// Inject a virtual key. Returns `false` if Windows refused the injection (e.g. UIPI).
#[cfg(windows)]
fn send_vk(vk: u16, down: bool) -> bool {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    let (scan, extended) = match KEYS.observed_scan(vk) {
        Some(s) => s,
        None => {
            let (s, ext) = layout_scan_ex(vk);
            (s, ext || is_extended_vk(vk))
        }
    };
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: OPENGEAR_INJECTED_TAG,
            },
        },
    };
    let ok = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) == 1 };
    if ok {
        INJECTED_OK_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        INJECTED_FAILED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

/// Inject a Snap Tap key by its physical scan code (`KEYEVENTF_SCANCODE`), including
/// `KEYEVENTF_EXTENDEDKEY` for arrow/nav keys, falling back to `send_vk` if scan == 0.
#[cfg(windows)]
fn send_pair_key(key: SocdKey, down: bool) -> bool {
    if key.scan == 0 {
        return send_vk(key.vk, down);
    }
    let mut flags = KEYEVENTF_SCANCODE;
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: key.scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: OPENGEAR_INJECTED_TAG,
            },
        },
    };
    let ok = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) == 1 };
    if ok {
        INJECTED_OK_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        INJECTED_FAILED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

#[cfg(windows)]
fn send_unicode_char(ch: char) {
    match ch {
        '\n' | '\r' => {
            send_vk(0x0D, true);
            send_vk(0x0D, false);
            return;
        }
        '\t' => {
            send_vk(0x09, true);
            send_vk(0x09, false);
            return;
        }
        _ => {}
    }
    let mut buf = [0u16; 2];
    let encoded = ch.encode_utf16(&mut buf);
    for &unit in encoded.iter() {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: OPENGEAR_INJECTED_TAG,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: OPENGEAR_INJECTED_TAG,
                    },
                },
            },
        ];
        unsafe {
            SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }
}

#[cfg(windows)]
enum MacroJob {
    Steps {
        steps: Vec<MacroStep>,
        generation: u32,
    },
    Text {
        text: String,
        delay_ms: u32,
        generation: u32,
    },
}

#[cfg(windows)]
fn macro_worker_tx() -> &'static Sender<MacroJob> {
    static TX: OnceLock<Sender<MacroJob>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<MacroJob>();
        let _ = thread::Builder::new()
            .name("opengear-macro-runner".to_string())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        MacroJob::Steps { steps, generation } => {
                            let mut pressed: Vec<u16> = Vec::new();
                            for step in steps {
                                if MACRO_GENERATION.load(Ordering::SeqCst) != generation {
                                    break;
                                }
                                if let Some(vk) = key_name_to_vk(&step.key)
                                    && send_vk(vk, step.down)
                                {
                                    track_pressed(&mut pressed, vk, step.down);
                                }
                                if step.delay_ms > 0
                                    && !sleep_unless_cancelled(
                                        capped_delay(step.delay_ms, MAX_MACRO_STEP_DELAY_MS),
                                        generation,
                                    )
                                {
                                    break;
                                }
                            }
                            for vk in pressed {
                                send_vk(vk, false);
                            }
                        }
                        MacroJob::Text {
                            text,
                            delay_ms,
                            generation,
                        } => {
                            for ch in text.chars() {
                                if MACRO_GENERATION.load(Ordering::SeqCst) != generation {
                                    break;
                                }
                                send_unicode_char(ch);
                                if delay_ms > 0
                                    && !sleep_unless_cancelled(
                                        capped_delay(delay_ms, MAX_TEXT_CHAR_DELAY_MS),
                                        generation,
                                    )
                                {
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        tx
    })
}

#[cfg(windows)]
fn run_macro_steps(steps: Vec<MacroStep>) {
    let generation = MACRO_GENERATION.load(Ordering::SeqCst);
    let _ = macro_worker_tx().send(MacroJob::Steps { steps, generation });
}

#[cfg(windows)]
fn run_text_macro(text: String, delay_ms: u32) {
    let generation = MACRO_GENERATION.load(Ordering::SeqCst);
    let _ = macro_worker_tx().send(MacroJob::Text {
        text,
        delay_ms,
        generation,
    });
}

/// Cancel running macros and release every key OpenGear is holding down on the user's behalf.
#[cfg(windows)]
fn release_all_injected_keys() {
    MACRO_GENERATION.fetch_add(1, Ordering::SeqCst);
    for target in KEYS.take_all_remaps() {
        send_vk(target, false);
    }
}

/// Release held remaps, clear tracked physical/SOCD/chatter/suppressed states, and cancel macros.
pub fn reset_held_state() {
    #[cfg(windows)]
    release_all_injected_keys();
    #[cfg(not(windows))]
    MACRO_GENERATION.fetch_add(1, Ordering::SeqCst);

    KEYS.clear_held();
    for slot in &EFFECTIVE_DOWN {
        slot.store(false, Ordering::Relaxed);
    }
    if let Ok(mut st) = state_store().lock() {
        st.socd.clear();
        st.chatter.clear();
    }
}

#[cfg(windows)]
fn keyboard_hook_inner(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let is_injected =
            (info.flags & LLKHF_INJECTED) != 0 || info.dwExtraInfo == OPENGEAR_INJECTED_TAG;
        if is_injected {
            note_effective(
                info.scanCode,
                wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize,
            );
            return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
        }

        let vk = info.vkCode as u16;
        let is_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
        let is_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

        if is_down || is_up {
            let now = Instant::now();
            let scan = info.scanCode as u16;

            // Snapshot rules under a short read lock, then drop before any state lock or SendInput.
            let (key_chatter_ms, win_lock, matched_pair, action) =
                if let Ok(rules) = rules_store().try_read() {
                    let pair = if rules.socd_enabled && !SOCD_SUSPENDED.load(Ordering::Relaxed) {
                        rules
                            .socd_pairs
                            .iter()
                            .find_map(|p| p.side_of(vk, scan).map(|side| (*p, side)))
                    } else {
                        None
                    };
                    (
                        rules.key_chatter_ms,
                        rules.win_lock,
                        pair,
                        rules.by_vk.get(&vk).cloned(),
                    )
                } else {
                    (0, false, None, None)
                };

            // 0. Software Anti-Chatter BEFORE note_physical, ripples, or macro recorder so
            //    bounced switches never pollute repeat detection, RGB ripples, or recordings.
            if key_chatter_ms > 0 {
                let blocked = state_store()
                    .try_lock()
                    .map(|mut st| st.chatter.check_key(vk, is_down, key_chatter_ms, now))
                    .unwrap_or(false);
                if blocked {
                    CHATTER_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
                    return 1;
                }
            }

            let is_repeat = KEYS.note_physical(vk, is_down);
            KEYS.note_scan(vk, info.scanCode, (info.flags & LLKHF_EXTENDED) != 0);

            // 1. Active remap release / repeat (guarantees remaps never stick if rules change mid-hold).
            if is_up {
                if let Some(target_vk) = KEYS.end_remap(vk) {
                    send_vk(target_vk, false);
                    return 1;
                }
            } else if let Some(target_vk) = KEYS.remap_target(vk) {
                send_vk(target_vk, true);
                return 1;
            }

            // 2. Reactive typing RGB ripple feed (fresh presses only, &'static str -> 0 allocs).
            if is_down
                && !is_repeat
                && let Some(kname) = vk_to_key_name(vk)
                && let Ok(mut q) = ripples_store().try_lock()
            {
                if q.len() >= 24 {
                    q.pop_front();
                }
                q.push_back((kname, now));
            }

            // 3. Live Macro Recorder capture (ignore OS key-repeat; drop lock before CallNextHookEx).
            let recording_active = if let Ok(mut rec) = recorder_store().try_lock() {
                if rec.recording {
                    if !(is_down && is_repeat)
                        && let Some(kname) = vk_to_key_name(vk)
                    {
                        let delay_ms = rec
                            .last_event
                            .map(|prev| {
                                now.saturating_duration_since(prev).as_millis().min(2000) as u32
                            })
                            .unwrap_or(0);
                        if let Some(prev_step) = rec.steps.last_mut() {
                            prev_step.delay_ms = delay_ms.max(5);
                        }
                        if rec.steps.len() < 128 {
                            rec.steps.push(MacroStep {
                                key: kname.to_string(),
                                down: is_down,
                                delay_ms: 10,
                            });
                        }
                        rec.last_event = Some(now);
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if recording_active {
                note_effective(info.scanCode, is_down);
                return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
            }

            // 4. WinLock check (track suppressed DOWN so UP is only swallowed when DOWN was)
            if win_lock && (vk == 0x5B || vk == 0x5C) {
                if is_down {
                    KEYS.mark_suppressed(vk, true);
                    return 1;
                } else if KEYS.take_suppressed(vk) {
                    return 1;
                }
            }

            // 5. SOCD / Snap Tap (state lock dropped BEFORE SendInput and CallNextHookEx)
            let mut post_inject: Option<(SocdKey, bool)> = None;
            if let Some((pair, is_a)) = matched_pair {
                let decision: Option<SocdDecision> = state_store()
                    .try_lock()
                    .ok()
                    .map(|mut st| st.socd.handle_event(&pair, is_a, is_down));
                if let Some(dec) = decision {
                    if dec.resolved_conflict {
                        SOCD_RESOLVED_COUNT.fetch_add(1, Ordering::Relaxed);
                    }
                    if let Some((pre_key, pre_down)) = dec.pre_inject {
                        send_pair_key(pre_key, pre_down);
                    }
                    if !dec.pass_current {
                        return 1;
                    }
                    post_inject = dec.post_inject;
                }
            }

            // 6. Custom key remaps, macros & text actions
            if let Some(action) = action {
                match action {
                    KeyAction::Disable => {
                        if is_down {
                            KEYS.mark_suppressed(vk, true);
                            return 1;
                        } else if KEYS.take_suppressed(vk) {
                            return 1;
                        }
                    }
                    KeyAction::Remap { target } => {
                        if is_down
                            && let Some(target_vk) = key_name_to_vk(&target)
                            && send_vk(target_vk, true)
                        {
                            KEYS.begin_remap(vk, target_vk);
                            return 1;
                        }
                    }
                    KeyAction::Macro { steps, .. } => {
                        if is_down && !is_repeat {
                            run_macro_steps(steps);
                        }
                        if is_down {
                            KEYS.mark_suppressed(vk, true);
                            return 1;
                        } else if KEYS.take_suppressed(vk) {
                            return 1;
                        }
                    }
                    KeyAction::Text { text, delay_ms } => {
                        if is_down && !is_repeat {
                            run_text_macro(text, delay_ms);
                        }
                        if is_down {
                            KEYS.mark_suppressed(vk, true);
                            return 1;
                        } else if KEYS.take_suppressed(vk) {
                            return 1;
                        }
                    }
                }
            }

            if is_down {
                KEYS.mark_suppressed(vk, false);
            } else {
                let _ = KEYS.take_suppressed(vk);
            }

            note_effective(info.scanCode, is_down);
            let res = unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
            if let Some((pk, pd)) = post_inject {
                send_pair_key(pk, pd);
            }
            return res;
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(windows)]
unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        keyboard_hook_inner(code, wparam, lparam)
    }))
    .unwrap_or_else(|_| unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) })
}

#[cfg(windows)]
fn mouse_hook_inner(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let wp = wparam as u32;
        if wp == WM_LBUTTONDOWN || wp == WM_LBUTTONUP || wp == WM_RBUTTONDOWN || wp == WM_RBUTTONUP
        {
            let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            let is_injected =
                (info.flags & LLMHF_INJECTED) != 0 || info.dwExtraInfo == OPENGEAR_INJECTED_TAG;
            if !is_injected {
                let debounce_ms = rules_store()
                    .try_read()
                    .map(|r| r.mouse_debounce_ms)
                    .unwrap_or(0);
                if debounce_ms > 0 {
                    let btn_idx = if wp == WM_LBUTTONDOWN || wp == WM_LBUTTONUP {
                        0
                    } else {
                        1
                    };
                    let is_down = wp == WM_LBUTTONDOWN || wp == WM_RBUTTONDOWN;
                    let now = Instant::now();
                    let blocked = state_store()
                        .try_lock()
                        .map(|mut st| st.chatter.check_mouse(btn_idx, is_down, debounce_ms, now))
                        .unwrap_or(false);
                    if blocked {
                        MOUSE_DBLCLICK_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
                        return 1;
                    }
                }
            }
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Ultra-lightweight mouse button anti-double-click hook.
/// NEVER inspects or modifies `WM_MOUSEMOVE` deltas (100% raw-input & anti-cheat safe).
#[cfg(windows)]
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mouse_hook_inner(code, wparam, lparam)
    }))
    .unwrap_or_else(|_| unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) })
}

/// Manages the background Windows low-level keyboard & optional mouse-debounce hooks
/// for remapping, WinLock, macros, SOCD/Snap Tap, anti-chatter, reactive RGB, and live recording.
pub struct InputEngine {
    thread_id: Arc<AtomicU32>,
    worker: Option<JoinHandle<()>>,
    /// Last failed hook installation (time, error), to rate-limit retries.
    last_start_failure: Option<(Instant, String)>,
}

impl Default for InputEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputEngine {
    pub fn new() -> Self {
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
            worker: None,
            last_start_failure: None,
        }
    }

    /// True only while the hook thread is alive with its keyboard hook installed.
    pub fn is_running(&self) -> bool {
        self.worker.as_ref().is_some_and(|h| !h.is_finished())
            && HOOK_ACTIVE.load(Ordering::Relaxed)
    }

    pub fn is_socd_suspended(&self) -> bool {
        SOCD_SUSPENDED.load(Ordering::Relaxed)
    }

    pub fn set_socd_suspended(&self, suspended: bool) {
        let prev = SOCD_SUSPENDED.swap(suspended, Ordering::SeqCst);
        if prev != suspended
            && let Ok(mut st) = state_store().lock()
        {
            st.socd.clear();
        }
    }

    /// (Snap Tap resolutions, injections accepted, injections refused by Windows).
    pub fn injection_stats(&self) -> (u32, u32, u32) {
        (
            SOCD_RESOLVED_COUNT.load(Ordering::Relaxed),
            INJECTED_OK_COUNT.load(Ordering::Relaxed),
            INJECTED_FAILED_COUNT.load(Ordering::Relaxed),
        )
    }

    pub fn stats(&self) -> (u32, u32) {
        (
            CHATTER_BLOCKED_COUNT.load(Ordering::Relaxed),
            MOUSE_DBLCLICK_BLOCKED_COUNT.load(Ordering::Relaxed),
        )
    }

    pub fn update_rules(
        &self,
        win_lock: bool,
        bindings: &BTreeMap<String, KeyAction>,
        socd: &SocdConfig,
        key_chatter_ms: u16,
        mouse_debounce_ms: u16,
    ) {
        let mut by_vk = HashMap::new();
        for (key_name, action) in bindings {
            if let Some(vk) = key_name_to_vk(key_name) {
                by_vk.insert(vk, action.clone());
            }
        }
        let mut socd_pairs = Vec::new();
        for p in &socd.pairs {
            if let (Some(va), Some(vb)) = (key_name_to_vk(&p.key_a), key_name_to_vk(&p.key_b))
                && va != vb
            {
                let preset = is_movement_preset_pair(&p.key_a, &p.key_b);
                let (scan_a, ext_a) = resolve_pair_scan(&p.key_a, va, preset);
                let (scan_b, ext_b) = resolve_pair_scan(&p.key_b, vb, preset);
                socd_pairs.push(SocdPair {
                    a: SocdKey {
                        vk: va,
                        scan: scan_a,
                        extended: ext_a,
                    },
                    b: SocdKey {
                        vk: vb,
                        scan: scan_b,
                        extended: ext_b,
                    },
                    mode: SocdMode::LastInputWins,
                });
            }
        }
        if let Ok(mut guard) = rules_store().write() {
            guard.win_lock = win_lock;
            guard.by_vk = by_vk;
            guard.socd_enabled = socd.enabled;
            guard.socd_pairs = socd_pairs;
            guard.key_chatter_ms = key_chatter_ms.min(80);
            guard.mouse_debounce_ms = mouse_debounce_ms.min(100);
        }
        if let Ok(mut st) = state_store().lock() {
            st.socd.clear();
        }
        // Dynamically attach/detach WH_MOUSE_LL so high-polling mice have ZERO overhead when disabled.
        #[cfg(windows)]
        {
            let tid = self.thread_id.load(Ordering::SeqCst);
            if tid != 0 {
                unsafe {
                    PostThreadMessageW(tid, WM_SYNC_MOUSE_HOOK, 0, 0);
                }
            }
        }
    }

    /// Start live keystroke macro recording (automatically starts the hook if needed).
    pub fn start_macro_recording(&mut self) -> anyhow::Result<()> {
        if !self.is_running() {
            self.start()?;
        }
        if let Ok(mut rec) = recorder_store().lock() {
            rec.steps.clear();
            rec.last_event = None;
            rec.recording = true;
        }
        Ok(())
    }

    /// Stop live macro recording and return captured steps.
    pub fn stop_macro_recording(&self) -> Vec<MacroStep> {
        if let Ok(mut rec) = recorder_store().lock() {
            rec.recording = false;
            rec.last_event = None;
            std::mem::take(&mut rec.steps)
        } else {
            Vec::new()
        }
    }

    pub fn is_recording_macro(&self) -> bool {
        recorder_store()
            .try_lock()
            .map(|r| r.recording)
            .unwrap_or(false)
    }

    /// Snapshot of keys pressed within the last `max_age_secs` for reactive RGB ripples.
    pub fn recent_key_ripples(&self, max_age_secs: f32) -> Vec<(String, f32)> {
        let now = Instant::now();
        let mut out = Vec::new();
        if let Ok(mut q) = ripples_store().try_lock() {
            while let Some((_, ts)) = q.front() {
                if now.saturating_duration_since(*ts).as_secs_f32() > max_age_secs {
                    q.pop_front();
                } else {
                    break;
                }
            }
            for (k, ts) in q.iter() {
                let age = now.saturating_duration_since(*ts).as_secs_f32();
                if age <= max_age_secs {
                    out.push(((*k).to_string(), age));
                }
            }
        }
        out
    }

    /// Install the low-level hooks on a dedicated thread and wait until the keyboard hook is
    /// actually in place.
    #[cfg(windows)]
    pub fn start(&mut self) -> anyhow::Result<()> {
        if let Some(h) = self.worker.as_ref() {
            if !h.is_finished() {
                HOOK_ACTIVE.store(true, Ordering::SeqCst);
                return Ok(());
            }
            if let Some(h) = self.worker.take() {
                let _ = h.join();
            }
        }
        if let Some((at, err)) = &self.last_start_failure
            && at.elapsed() < START_RETRY_BACKOFF
        {
            anyhow::bail!("{err}");
        }

        let tid_slot = self.thread_id.clone();
        tid_slot.store(0, Ordering::SeqCst);
        KEYS.clear_held();
        HOOK_ACTIVE.store(true, Ordering::SeqCst);
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let handle = thread::Builder::new()
            .name("opengear-input-hook".to_string())
            .spawn(move || unsafe {
                let mut msg: MSG = std::mem::zeroed();
                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                let tid = GetCurrentThreadId();
                tid_slot.store(tid, Ordering::SeqCst);

                let kb_hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(keyboard_hook_proc),
                    std::ptr::null_mut(),
                    0,
                );
                if kb_hook.is_null() {
                    let err = format!(
                        "SetWindowsHookExW(WH_KEYBOARD_LL) a échoué (code {})",
                        GetLastError()
                    );
                    error!("{err}");
                    HOOK_ACTIVE.store(false, Ordering::SeqCst);
                    let _ = ready_tx.send(Err(err));
                    return;
                }
                let want_mouse = rules_store()
                    .read()
                    .map(|r| r.mouse_debounce_ms > 0)
                    .unwrap_or(false);
                let mut mouse_hook: HHOOK = if want_mouse {
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), std::ptr::null_mut(), 0)
                } else {
                    std::ptr::null_mut()
                };
                info!(
                    "Moteur Input/SOCD/Debounce Windows démarré (TID={tid}, mouse_hook={})",
                    !mouse_hook.is_null()
                );
                let _ = ready_tx.send(Ok(()));

                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    if msg.message == WM_SYNC_MOUSE_HOOK {
                        let need = rules_store()
                            .read()
                            .map(|r| r.mouse_debounce_ms > 0)
                            .unwrap_or(false);
                        if need && mouse_hook.is_null() {
                            mouse_hook = SetWindowsHookExW(
                                WH_MOUSE_LL,
                                Some(mouse_hook_proc),
                                std::ptr::null_mut(),
                                0,
                            );
                            info!("Bouclier Anti-Double-Clic Souris (WH_MOUSE_LL) activé");
                        } else if !need && !mouse_hook.is_null() {
                            UnhookWindowsHookEx(mouse_hook);
                            mouse_hook = std::ptr::null_mut();
                            info!("Bouclier Souris (WH_MOUSE_LL) détaché — 0 overhead souris");
                        }
                        continue;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                UnhookWindowsHookEx(kb_hook);
                if !mouse_hook.is_null() {
                    UnhookWindowsHookEx(mouse_hook);
                }
                HOOK_ACTIVE.store(false, Ordering::SeqCst);
                info!("Moteur Input/SOCD arrêté proprement");
            })?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {
                self.worker = Some(handle);
                self.last_start_failure = None;
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = handle.join();
                HOOK_ACTIVE.store(false, Ordering::SeqCst);
                self.last_start_failure = Some((Instant::now(), err.clone()));
                anyhow::bail!(err)
            }
            Err(_) => {
                self.worker = Some(handle);
                let err =
                    "Le hook clavier n'a pas confirmé son installation (délai dépassé)".to_string();
                self.last_start_failure = Some((Instant::now(), err.clone()));
                anyhow::bail!(err)
            }
        }
    }

    #[cfg(not(windows))]
    pub fn start(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("InputEngine requiert Windows");
    }

    /// Stop the hooks: cancel running macros, release held/suppressed keys, and ask the hook
    /// thread to quit.
    #[cfg(windows)]
    pub fn stop(&mut self) {
        HOOK_ACTIVE.store(false, Ordering::SeqCst);
        reset_held_state();
        if let Some(handle) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !handle.is_finished() && Instant::now() < deadline {
                let tid = self.thread_id.load(Ordering::SeqCst);
                if tid != 0 {
                    unsafe {
                        PostThreadMessageW(tid, WM_QUIT, 0, 0);
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                error!("Le thread du hook clavier ne s'est pas arrêté à temps — détaché");
            }
        }
        self.thread_id.store(0, Ordering::SeqCst);
    }

    #[cfg(not(windows))]
    pub fn stop(&mut self) {
        reset_held_state();
    }
}

impl Drop for InputEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_state_tracks_movement_keys_only() {
        note_effective(0x1E, true);
        note_effective(0x20, true);
        note_effective(0x1E, false);
        note_effective(0x10, true);
        let k = movement_keys_effective();
        assert!(!k[0] && k[1] && !k[2] && !k[3]);
        note_effective(0x20, false);
    }

    #[test]
    fn movement_preset_overrides_apply_only_to_movement_pairs() {
        assert!(is_movement_preset_pair("Q", "D"));
        assert!(is_movement_preset_pair("a", "d"));
        assert!(is_movement_preset_pair("Z", "S"));
        assert!(is_movement_preset_pair("w", "s"));
        assert!(!is_movement_preset_pair("Q", "E"));
        assert!(!is_movement_preset_pair("LEFT", "RIGHT"));

        assert_eq!(movement_preset_scan("Q"), Some((0x1E, false)));
        assert_eq!(movement_preset_scan("A"), Some((0x1E, false)));
        assert_eq!(movement_preset_scan("d"), Some((0x20, false)));
        assert_eq!(movement_preset_scan("Z"), Some((0x11, false)));
        assert_eq!(movement_preset_scan("W"), Some((0x11, false)));
    }

    #[test]
    fn decode_vsc_ex_extracts_extended_arrows() {
        assert_eq!(decode_vsc_ex(0xE04B), (0x4B, true));
        assert_eq!(decode_vsc_ex(0x001E), (0x1E, false));
    }

    #[test]
    fn snap_tap_pairs_match_physical_keys_whatever_the_layout() {
        let pair = SocdPair {
            a: SocdKey {
                vk: 0x51,
                scan: 0x1E,
                extended: false,
            },
            b: SocdKey {
                vk: 0x44,
                scan: 0x20,
                extended: false,
            },
            mode: SocdMode::LastInputWins,
        };
        assert_eq!(pair.side_of(0x41, 0x1E), Some(true));
        assert_eq!(pair.side_of(0x00, 0x20), Some(false));
        assert_eq!(pair.side_of(0x51, 0), Some(true));
        assert_eq!(pair.side_of(0x45, 0x12), None);
    }

    #[test]
    fn observed_scan_code_is_reused_with_its_extended_flag() {
        let k = KeyTracker::new();
        assert_eq!(k.observed_scan(0x51), None);
        k.note_scan(0x51, 0x1E, false);
        assert_eq!(k.observed_scan(0x51), Some((0x1E, false)));
        k.note_scan(0x51, 0, false);
        assert_eq!(k.observed_scan(0x51), Some((0x1E, false)));
        k.note_scan(0x25, 0x4B, true);
        assert_eq!(k.observed_scan(0x25), Some((0x4B, true)));
    }

    #[test]
    fn suppressed_keys_release_only_when_down_was_swallowed() {
        let k = KeyTracker::new();
        assert!(!k.take_suppressed(0x5B));
        k.mark_suppressed(0x5B, true);
        assert!(k.take_suppressed(0x5B));
        assert!(!k.take_suppressed(0x5B));
    }

    #[test]
    fn auto_repeat_is_detected_only_while_held() {
        let k = KeyTracker::new();
        assert!(!k.note_physical(0x41, true));
        assert!(k.note_physical(0x41, true));
        assert!(k.note_physical(0x41, true));
        assert!(!k.note_physical(0x41, false));
        assert!(!k.note_physical(0x41, true));
    }

    #[test]
    fn clear_held_forgets_keys_released_while_stopped() {
        let k = KeyTracker::new();
        k.note_physical(0x20, true);
        k.mark_suppressed(0x5B, true);
        k.clear_held();
        assert!(!k.note_physical(0x20, true));
        assert!(!k.take_suppressed(0x5B));
    }

    #[test]
    fn out_of_range_vk_is_ignored() {
        let k = KeyTracker::new();
        assert!(!k.note_physical(300, true));
        assert!(!k.note_physical(300, true));
        k.begin_remap(300, 0x41);
        assert_eq!(k.end_remap(300), None);
    }

    #[test]
    fn remap_release_returns_the_target_pressed_at_keydown() {
        let k = KeyTracker::new();
        k.begin_remap(0x14, 0x7C);
        assert_eq!(k.remap_target(0x14), Some(0x7C));
        assert_eq!(k.end_remap(0x14), Some(0x7C));
        assert_eq!(k.end_remap(0x14), None);
        assert_eq!(k.remap_target(0x14), None);
    }

    #[test]
    fn stop_collects_every_held_remap_target() {
        let k = KeyTracker::new();
        k.begin_remap(0x14, 0x7C);
        k.begin_remap(0x91, 0xB3);
        let mut all = k.take_all_remaps();
        all.sort_unstable();
        assert_eq!(all, vec![0x7C, 0xB3]);
        assert!(k.take_all_remaps().is_empty());
    }

    #[test]
    fn macro_tracks_keys_left_down() {
        let mut pressed = Vec::new();
        track_pressed(&mut pressed, 0xA2, true);
        track_pressed(&mut pressed, 0x43, true);
        track_pressed(&mut pressed, 0x43, true);
        track_pressed(&mut pressed, 0x43, false);
        assert_eq!(pressed, vec![0xA2]);
    }

    #[test]
    fn delays_are_capped() {
        assert_eq!(
            capped_delay(50, MAX_MACRO_STEP_DELAY_MS),
            Duration::from_millis(50)
        );
        assert_eq!(
            capped_delay(u32::MAX, MAX_MACRO_STEP_DELAY_MS),
            Duration::from_millis(MAX_MACRO_STEP_DELAY_MS as u64)
        );
        assert_eq!(
            capped_delay(60_000, MAX_TEXT_CHAR_DELAY_MS),
            Duration::from_millis(MAX_TEXT_CHAR_DELAY_MS as u64)
        );
    }

    #[test]
    fn cancelled_sleep_returns_early() {
        let generation = MACRO_GENERATION.load(Ordering::SeqCst);
        let started = Instant::now();
        let canceller = thread::spawn(|| {
            thread::sleep(Duration::from_millis(50));
            MACRO_GENERATION.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!sleep_unless_cancelled(Duration::from_secs(5), generation));
        assert!(started.elapsed() < Duration::from_secs(2));
        canceller.join().unwrap();
        let g = MACRO_GENERATION.load(Ordering::SeqCst);
        assert!(sleep_unless_cancelled(Duration::from_millis(5), g));
    }
}
