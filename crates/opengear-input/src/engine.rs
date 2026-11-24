use crate::keys::{key_name_to_vk, vk_to_key_name};
use opengear_core::{KeyAction, MacroStep, SocdConfig};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
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
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    GetKeyboardLayout, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyExW,
    SendInput,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const OPENGEAR_INJECTED_TAG: usize = 0x4F47_4541;
const LLMHF_INJECTED: u32 = 0x0000_0001;
#[cfg(windows)]
const WM_SYNC_MOUSE_HOOK: u32 = WM_APP + 77;

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
/// (which must never block): physical held state and active remaps.
pub(crate) struct KeyTracker {
    /// Physically held keys as seen by the hook (non-injected events only).
    held: [AtomicBool; 256],
    /// For a source key currently remapped: the target VK OpenGear pressed (0 = none).
    remapped_to: [AtomicU16; 256],
    /// Last hardware scan code seen for each VK (bit 15 = extended key), 0 = never seen.
    /// Re-injecting a key with the scan code the user's keyboard really produced keeps the
    /// physical key identity right whatever the keyboard layout (AZERTY vs QWERTY...).
    scan: [AtomicU16; 256],
}

impl KeyTracker {
    pub(crate) const fn new() -> Self {
        Self {
            held: [const { AtomicBool::new(false) }; 256],
            remapped_to: [const { AtomicU16::new(0) }; 256],
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

    /// Forget every held key (events are not observed while the engine is stopped).
    pub(crate) fn clear_held(&self) {
        for h in &self.held {
            h.store(false, Ordering::SeqCst);
        }
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

#[derive(Debug, Clone)]
struct SocdPairRuntime {
    vk_a: u16,
    vk_b: u16,
    /// Physical keys (scan codes) of the pair. Snap Tap is about physical positions
    /// (left/right, forward/back): matching on scan codes works whatever the keyboard layout
    /// and whatever virtual-key code Windows reports to the hook.
    scan_a: u16,
    scan_b: u16,
}

impl SocdPairRuntime {
    /// Which side of the pair a key event belongs to: `Some(true)` = A, `Some(false)` = B.
    /// Scan code first (physical position), virtual-key code as a fallback.
    fn side_of(&self, vk: u16, scan: u16) -> Option<bool> {
        if scan != 0 {
            if scan == self.scan_a {
                return Some(true);
            }
            if scan == self.scan_b {
                return Some(false);
            }
        }
        if vk == self.vk_a {
            Some(true)
        } else if vk == self.vk_b {
            Some(false)
        } else {
            None
        }
    }
}

/// Physical scan code for a Snap Tap key name.
///
/// The Snap Tap presets always target the movement cluster (the WASD positions), named by
/// their AZERTY (Q D Z S) or QWERTY (A D W S) letters: those map to fixed set-1 scan codes,
/// independent of the layout Windows applies. Other keys use the active layout.
pub(crate) fn scan_for_name(name: &str, vk: u16) -> u16 {
    match name.trim().to_ascii_uppercase().as_str() {
        "Q" | "A" => 0x1E, // left
        "D" => 0x20,       // right
        "Z" | "W" => 0x11, // forward
        "S" => 0x1F,       // back
        _ => layout_scan(vk),
    }
}

#[cfg(windows)]
fn layout_scan(vk: u16) -> u16 {
    scan_for_foreground_layout(vk)
}

#[cfg(not(windows))]
fn layout_scan(_vk: u16) -> u16 {
    0
}

/// Lock-free ring of the last raw keyboard events seen by the hook (diagnostics).
/// Packed: vk (8 bits) | scan (12 bits) | injected (1) | pair match (1).
const DBG_RING_LEN: usize = 12;
static DBG_RING: [AtomicU32; DBG_RING_LEN] = [const { AtomicU32::new(0) }; DBG_RING_LEN];
static DBG_RING_POS: AtomicU32 = AtomicU32::new(0);

fn dbg_record_event(vk: u32, scan: u32, injected: bool, pair: bool, down: bool) {
    let packed = (vk & 0xFF)
        | ((scan & 0xFFF) << 8)
        | (u32::from(injected) << 20)
        | (u32::from(pair) << 21)
        | (u32::from(down) << 22)
        | (1 << 31);
    let i = DBG_RING_POS.fetch_add(1, Ordering::Relaxed) as usize % DBG_RING_LEN;
    DBG_RING[i].store(packed, Ordering::Relaxed);
}

/// Last raw events, newest first, human readable.
fn dbg_recent_events() -> String {
    let pos = DBG_RING_POS.load(Ordering::Relaxed) as usize;
    (0..DBG_RING_LEN)
        .filter_map(|k| {
            let i = (pos + DBG_RING_LEN - 1 - k) % DBG_RING_LEN;
            let v = DBG_RING[i].load(Ordering::Relaxed);
            (v & (1 << 31) != 0).then(|| {
                format!(
                    "{} vk=0x{:02X}({}) sc=0x{:02X}{}{}",
                    if v & (1 << 22) != 0 { "↓" } else { "↑" },
                    v & 0xFF,
                    vk_to_key_name((v & 0xFF) as u16).unwrap_or("?"),
                    (v >> 8) & 0xFFF,
                    if v & (1 << 20) != 0 { " injecté" } else { "" },
                    if v & (1 << 21) != 0 { " PAIRE" } else { "" }
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" ; ")
}

#[derive(Debug, Default, Clone)]
struct RuntimeRules {
    win_lock: bool,
    by_vk: HashMap<u16, KeyAction>,
    socd_enabled: bool,
    socd_pairs: Vec<SocdPairRuntime>,
    key_chatter_ms: u16,
    mouse_debounce_ms: u16,
}

#[derive(Debug, Default)]
struct SocdState {
    /// Physical held state per pair: (a_held, b_held, active_vk)
    pair_state: HashMap<(u16, u16), (bool, bool, Option<u16>)>,
    /// Last key release timestamp per VK for anti-chatter filtering
    last_key_up: HashMap<u16, Instant>,
    /// Last mouse button release timestamp (0 = LMB, 1 = RMB)
    last_mouse_up: [Option<Instant>; 2],
}

#[derive(Debug, Default)]
struct MacroRecorderState {
    recording: bool,
    last_event: Option<Instant>,
    steps: Vec<MacroStep>,
}

static GLOBAL_RULES: OnceLock<RwLock<RuntimeRules>> = OnceLock::new();
static RUNTIME_STATE: OnceLock<Mutex<SocdState>> = OnceLock::new();
static MACRO_RECORDER: OnceLock<Mutex<MacroRecorderState>> = OnceLock::new();
static REACTIVE_RIPPLES: OnceLock<Mutex<VecDeque<(String, Instant)>>> = OnceLock::new();

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static SOCD_SUSPENDED: AtomicBool = AtomicBool::new(false);
static CHATTER_BLOCKED_COUNT: AtomicU32 = AtomicU32::new(0);
/// Diagnostics: Snap Tap resolutions performed, and key injections accepted / refused by Windows.
static SOCD_RESOLVED_COUNT: AtomicU32 = AtomicU32::new(0);
/// Hook pipeline diagnostics (see `InputEngine::debug_counters`).
static DBG_HOOK_EVENTS: AtomicU32 = AtomicU32::new(0);
static DBG_INJECTED_SKIPPED: AtomicU32 = AtomicU32::new(0);
static DBG_FOREIGN_INJECTED: AtomicU32 = AtomicU32::new(0);
static DBG_RECORDER_BYPASS: AtomicU32 = AtomicU32::new(0);
static DBG_RULES_BUSY: AtomicU32 = AtomicU32::new(0);
static DBG_SOCD_PAIR_KEYS: AtomicU32 = AtomicU32::new(0);
static DBG_SOCD_DISABLED: AtomicU32 = AtomicU32::new(0);
static DBG_SOCD_SUSPENDED: AtomicU32 = AtomicU32::new(0);
static DBG_SOCD_STATE_BUSY: AtomicU32 = AtomicU32::new(0);
static DBG_LAST_VK: AtomicU32 = AtomicU32::new(0);
static INJECTED_OK_COUNT: AtomicU32 = AtomicU32::new(0);
static INJECTED_FAILED_COUNT: AtomicU32 = AtomicU32::new(0);
static MOUSE_DBLCLICK_BLOCKED_COUNT: AtomicU32 = AtomicU32::new(0);

fn rules_store() -> &'static RwLock<RuntimeRules> {
    GLOBAL_RULES.get_or_init(|| RwLock::new(RuntimeRules::default()))
}

fn state_store() -> &'static Mutex<SocdState> {
    RUNTIME_STATE.get_or_init(|| Mutex::new(SocdState::default()))
}

fn recorder_store() -> &'static Mutex<MacroRecorderState> {
    MACRO_RECORDER.get_or_init(|| Mutex::new(MacroRecorderState::default()))
}

fn ripples_store() -> &'static Mutex<VecDeque<(String, Instant)>> {
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

/// Inject a virtual key. Returns `false` if Windows refused the injection (e.g. UIPI: the
/// foreground window runs elevated while OpenGear does not).
#[cfg(windows)]
fn send_vk(vk: u16, down: bool) -> bool {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    // Fill the hardware scan code too: apps that read physical keys (Chromium/WebView2 `code`,
    // Raw Input / DirectInput games) identify a key by its scan code. Prefer the scan code the
    // user's keyboard actually produced for this VK (always known for Snap Tap: the opposing key
    // was physically pressed); otherwise translate with the FOREGROUND window's layout — the
    // hook thread's own layout may differ (e.g. US on an AZERTY desktop) and give the wrong key.
    let (scan, extended) = match KEYS.observed_scan(vk) {
        Some(s) => s,
        None => (scan_for_foreground_layout(vk), is_extended_vk(vk)),
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

/// Inject a Snap Tap key by its physical scan code (`KEYEVENTF_SCANCODE`): Windows derives the
/// virtual key with the receiving window's layout, games reading scan codes see the right key.
/// Falls back to the virtual key when no scan code is known.
#[cfg(windows)]
fn send_pair_key(scan: u16, vk: u16, down: bool) -> bool {
    if scan == 0 {
        return send_vk(vk, down);
    }
    let mut flags = KEYEVENTF_SCANCODE;
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
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

/// VK -> scan code using the keyboard layout of the window that will receive the key.
#[cfg(windows)]
fn scan_for_foreground_layout(vk: u16) -> u16 {
    unsafe {
        let fg = GetForegroundWindow();
        let tid = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        };
        let hkl = GetKeyboardLayout(tid);
        MapVirtualKeyExW(vk as u32, MAPVK_VK_TO_VSC, hkl) as u16
    }
}

#[cfg(windows)]
fn send_unicode_char(ch: char) {
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

/// Play a macro on a worker thread. It stops early when the engine stops, and always releases
/// every key it pressed but did not release (cancelled run, or a recording truncated between a
/// key-down and its key-up), so a macro can never leave a key stuck down.
#[cfg(windows)]
fn run_macro_steps(steps: Vec<MacroStep>) {
    let generation = MACRO_GENERATION.load(Ordering::SeqCst);
    thread::spawn(move || {
        let mut pressed: Vec<u16> = Vec::new();
        for step in steps {
            if MACRO_GENERATION.load(Ordering::SeqCst) != generation {
                break;
            }
            if let Some(vk) = key_name_to_vk(&step.key) {
                if send_vk(vk, step.down) {
                    track_pressed(&mut pressed, vk, step.down);
                }
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
    });
}

#[cfg(windows)]
fn run_text_macro(text: String, delay_ms: u32) {
    let generation = MACRO_GENERATION.load(Ordering::SeqCst);
    thread::spawn(move || {
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

#[cfg(windows)]
unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let is_injected =
            (info.flags & LLKHF_INJECTED) != 0 || info.dwExtraInfo == OPENGEAR_INJECTED_TAG;
        DBG_HOOK_EVENTS.fetch_add(1, Ordering::Relaxed);
        DBG_LAST_VK.store(info.vkCode, Ordering::Relaxed);
        if is_injected {
            dbg_record_event(
                info.vkCode,
                info.scanCode,
                true,
                false,
                wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize,
            );
            DBG_INJECTED_SKIPPED.fetch_add(1, Ordering::Relaxed);
            if info.dwExtraInfo != OPENGEAR_INJECTED_TAG {
                // Injected by ANOTHER program (macro tool, keyboard remapper...)
                DBG_FOREIGN_INJECTED.fetch_add(1, Ordering::Relaxed);
            }
        }

        if !is_injected {
            let vk = info.vkCode as u16;
            let is_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
            let is_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

            if is_down || is_up {
                let now = Instant::now();
                let is_repeat = KEYS.note_physical(vk, is_down);
                KEYS.note_scan(vk, info.scanCode, (info.flags & LLKHF_EXTENDED) != 0);

                // 0. Active remap: undo / repeat exactly what was pressed on key-down, whatever the
                //    rules are now (they may have changed mid-press, or be write-locked right now).
                //    This is what guarantees a remapped target can never stay stuck down.
                if is_up {
                    if let Some(target_vk) = KEYS.end_remap(vk) {
                        send_vk(target_vk, false);
                        return 1;
                    }
                } else if let Some(target_vk) = KEYS.remap_target(vk) {
                    send_vk(target_vk, true);
                    return 1;
                }

                // 1. Reactive typing RGB ripple feed (non-blocking, fresh presses only)
                if is_down && !is_repeat {
                    if let Some(kname) = vk_to_key_name(vk) {
                        if let Ok(mut q) = ripples_store().try_lock() {
                            if q.len() >= 24 {
                                q.pop_front();
                            }
                            q.push_back((kname.to_string(), now));
                        }
                    }
                }

                // 2. Live Macro Recorder capture (never swallows keys so user can stop cleanly)
                if let Ok(mut rec) = recorder_store().try_lock() {
                    if rec.recording {
                        if let Some(kname) = vk_to_key_name(vk) {
                            let delay_ms = rec
                                .last_event
                                .map(|prev| now.saturating_duration_since(prev).as_millis().min(2000) as u32)
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
                        // While recording, don't execute conflicting remaps
                        DBG_RECORDER_BYPASS.fetch_add(1, Ordering::Relaxed);
                        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
                    }
                }

                if let Ok(rules) = rules_store().try_read() {
                    // 3. Software Anti-Chatter / Key Debounce Shield (only when configured > 0 ms)
                    if rules.key_chatter_ms > 0 {
                        if let Ok(mut st) = state_store().try_lock() {
                            if is_down {
                                if let Some(prev_up) = st.last_key_up.get(&vk) {
                                    if now.saturating_duration_since(*prev_up).as_millis()
                                        < rules.key_chatter_ms as u128
                                    {
                                        CHATTER_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
                                        return 1;
                                    }
                                }
                            } else if is_up {
                                st.last_key_up.insert(vk, now);
                            }
                        }
                    }

                    // 4. WinLock check
                    if rules.win_lock && (vk == 0x5B || vk == 0x5C) {
                        return 1;
                    }

                    // 5. SOCD / Snap Tap (Last-Input Priority) — automatically bypassed if suspended (e.g. cs2.exe)
                    //    Keys are matched by PHYSICAL position (scan code), VK as a fallback.
                    let scan = info.scanCode as u16;
                    let pair_hit = rules.socd_pairs.iter().any(|p| p.side_of(vk, scan).is_some());
                    dbg_record_event(info.vkCode, info.scanCode, false, pair_hit, is_down);
                    if pair_hit {
                        DBG_SOCD_PAIR_KEYS.fetch_add(1, Ordering::Relaxed);
                        if !rules.socd_enabled {
                            DBG_SOCD_DISABLED.fetch_add(1, Ordering::Relaxed);
                        } else if SOCD_SUSPENDED.load(Ordering::Relaxed) {
                            DBG_SOCD_SUSPENDED.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if rules.socd_enabled && !SOCD_SUSPENDED.load(Ordering::Relaxed) {
                        if let Some((pair, is_a)) = rules
                            .socd_pairs
                            .iter()
                            .find_map(|p| p.side_of(vk, scan).map(|side| (p, side)))
                        {
                            let key = (pair.vk_a, pair.vk_b);
                            // The opposing key is re-injected by its physical scan code.
                            let opp_scan = if is_a { pair.scan_b } else { pair.scan_a };
                            let opp_vk = if is_a { pair.vk_b } else { pair.vk_a };
                            // Identify sides by the pair's own VKs, not by the VK the hook
                            // reported (which may differ from the configured one).
                            let self_vk = if is_a { pair.vk_a } else { pair.vk_b };

                            if let Ok(mut st) = state_store().try_lock() {
                                let entry = st.pair_state.entry(key).or_insert((false, false, None));
                                if is_down {
                                    let was_held = if is_a { entry.0 } else { entry.1 };
                                    if is_a {
                                        entry.0 = true;
                                    } else {
                                        entry.1 = true;
                                    }
                                    // Ignore OS key-repeat if already active
                                    if !was_held {
                                        if entry.2 == Some(opp_vk) {
                                            // Release opposing key before letting new key pass
                                            send_pair_key(opp_scan, opp_vk, false);
                                            SOCD_RESOLVED_COUNT.fetch_add(1, Ordering::Relaxed);
                                        }
                                        entry.2 = Some(self_vk);
                                    } else if entry.2 != Some(self_vk) {
                                        // Suppress OS auto-repeat of suppressed key while opponent owns priority
                                        return 1;
                                    }
                                } else if is_up {
                                    if is_a {
                                        entry.0 = false;
                                    } else {
                                        entry.1 = false;
                                    }
                                    let opp_held = if is_a { entry.1 } else { entry.0 };
                                    if entry.2 == Some(self_vk) {
                                        if opp_held {
                                            // Re-engage held opposing key immediately!
                                            entry.2 = Some(opp_vk);
                                            // Let this key's UP pass, then press opponent
                                            let res = unsafe {
                                                CallNextHookEx(
                                                    std::ptr::null_mut(),
                                                    code,
                                                    wparam,
                                                    lparam,
                                                )
                                            };
                                            send_pair_key(opp_scan, opp_vk, true);
                                            return res;
                                        } else {
                                            entry.2 = None;
                                        }
                                    }
                                }
                            } else {
                                DBG_SOCD_STATE_BUSY.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }

                    // 6. Custom key remaps, macros & text actions
                    if let Some(action) = rules.by_vk.get(&vk) {
                        match action {
                            KeyAction::Disable => return 1,
                            KeyAction::Remap { target } => {
                                // Key-up of a tracked remap was already handled in step 0. An
                                // untracked key-up means the original key-down went through
                                // unchanged: let its key-up through too so it can't stay stuck.
                                if is_down {
                                    if let Some(target_vk) = key_name_to_vk(target) {
                                        if send_vk(target_vk, true) {
                                            KEYS.begin_remap(vk, target_vk);
                                            return 1;
                                        }
                                        // Injection refused (e.g. elevated foreground window):
                                        // pass the original key rather than swallowing it.
                                    }
                                }
                            }
                            KeyAction::Macro { steps, .. } => {
                                // One run per physical press: OS auto-repeat must not relaunch it.
                                if is_down && !is_repeat {
                                    run_macro_steps(steps.clone());
                                }
                                return 1;
                            }
                            KeyAction::Text { text, delay_ms } => {
                                if is_down && !is_repeat {
                                    run_text_macro(text.clone(), *delay_ms);
                                }
                                return 1;
                            }
                        }
                    }
                } else {
                    DBG_RULES_BUSY.fetch_add(1, Ordering::Relaxed);
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
    if code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let wp = wparam as u32;
        if wp == WM_LBUTTONDOWN || wp == WM_LBUTTONUP || wp == WM_RBUTTONDOWN || wp == WM_RBUTTONUP {
            let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            let is_injected =
                (info.flags & LLMHF_INJECTED) != 0 || info.dwExtraInfo == OPENGEAR_INJECTED_TAG;
            if !is_injected {
                let debounce_ms = rules_store()
                    .try_read()
                    .map(|r| r.mouse_debounce_ms)
                    .unwrap_or(0);
                if debounce_ms > 0 {
                    let btn_idx = if wp == WM_LBUTTONDOWN || wp == WM_LBUTTONUP { 0 } else { 1 };
                    let is_down = wp == WM_LBUTTONDOWN || wp == WM_RBUTTONDOWN;
                    let now = Instant::now();
                    if let Ok(mut st) = state_store().try_lock() {
                        if is_down {
                            if let Some(prev_up) = st.last_mouse_up[btn_idx] {
                                if now.saturating_duration_since(prev_up).as_millis()
                                    < debounce_ms as u128
                                {
                                    MOUSE_DBLCLICK_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
                                    return 1;
                                }
                            }
                        } else {
                            st.last_mouse_up[btn_idx] = Some(now);
                        }
                    }
                }
            }
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
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
        if prev != suspended {
            if let Ok(mut st) = state_store().lock() {
                st.pair_state.clear();
            }
        }
    }

    /// Step-by-step counters of the keyboard hook pipeline, plus the runtime SOCD rules, to tell
    /// exactly why Snap Tap does or does not act (hook not called, keys seen as injected by another
    /// program, rules not synced, suspended, state busy...).
    pub fn debug_counters(&self) -> Vec<(&'static str, String)> {
        let rules = rules_store().read().ok();
        let (socd_on, pairs) = rules
            .as_ref()
            .map(|r| {
                (
                    r.socd_enabled,
                    r.socd_pairs
                        .iter()
                        .map(|p| {
                            format!(
                                "{}+{}(sc {:02X}/{:02X})",
                                vk_to_key_name(p.vk_a).unwrap_or("?"),
                                vk_to_key_name(p.vk_b).unwrap_or("?"),
                                p.scan_a,
                                p.scan_b
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .unwrap_or((false, String::from("(règles illisibles)")));
        let recording = recorder_store().lock().map(|r| r.recording).unwrap_or(false);
        let c = |a: &AtomicU32| a.load(Ordering::Relaxed).to_string();
        vec![
            ("hook_actif", self.is_running().to_string()),
            ("evenements_vus", c(&DBG_HOOK_EVENTS)),
            ("injectes_ignores", c(&DBG_INJECTED_SKIPPED)),
            ("injectes_par_autre_logiciel", c(&DBG_FOREIGN_INJECTED)),
            ("derniere_touche_vk", format!("0x{:02X}", DBG_LAST_VK.load(Ordering::Relaxed))),
            ("enregistreur_macro", recording.to_string()),
            ("ignores_enregistreur", c(&DBG_RECORDER_BYPASS)),
            ("regles_occupees", c(&DBG_RULES_BUSY)),
            ("snaptap_regle_active", socd_on.to_string()),
            ("snaptap_paires", pairs),
            ("snaptap_en_pause", SOCD_SUSPENDED.load(Ordering::Relaxed).to_string()),
            ("touches_de_paire_vues", c(&DBG_SOCD_PAIR_KEYS)),
            ("paire_mais_desactive", c(&DBG_SOCD_DISABLED)),
            ("paire_mais_en_pause", c(&DBG_SOCD_SUSPENDED)),
            ("etat_occupe", c(&DBG_SOCD_STATE_BUSY)),
            ("snaptap_resolutions", c(&SOCD_RESOLVED_COUNT)),
            ("injections_ok", c(&INJECTED_OK_COUNT)),
            ("injections_refusees", c(&INJECTED_FAILED_COUNT)),
            ("evenements_recents", dbg_recent_events()),
        ]
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
            if let (Some(va), Some(vb)) = (key_name_to_vk(&p.key_a), key_name_to_vk(&p.key_b)) {
                if va != vb {
                    // Physical positions of the named keys in the user's active layout
                    // (AZERTY "Q" = the key at scan 0x1E, QWERTY "A" = same physical key).
                    socd_pairs.push(SocdPairRuntime {
                        vk_a: va,
                        vk_b: vb,
                        scan_a: scan_for_name(&p.key_a, va),
                        scan_b: scan_for_name(&p.key_b, vb),
                    });
                }
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
            st.pair_state.clear();
        }
        // Dynamically attach/detach WH_MOUSE_LL so 1000-8000 Hz mice have ZERO hook overhead when mouse_debounce_ms == 0!
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
                    out.push((k.clone(), age));
                }
            }
        }
        out
    }

    /// Install the low-level hooks on a dedicated thread and wait until the keyboard hook is
    /// actually in place: returns an error (instead of a false "running" state) if Windows
    /// refuses it. Failed attempts are not retried more often than every few seconds.
    #[cfg(windows)]
    pub fn start(&mut self) -> anyhow::Result<()> {
        if let Some(h) = self.worker.as_ref() {
            if !h.is_finished() {
                HOOK_ACTIVE.store(true, Ordering::SeqCst);
                return Ok(());
            }
            // The previous hook thread exited (hook failure or WM_QUIT): reap it and restart.
            if let Some(h) = self.worker.take() {
                let _ = h.join();
            }
        }
        if let Some((at, err)) = &self.last_start_failure {
            if at.elapsed() < START_RETRY_BACKOFF {
                anyhow::bail!("{err}");
            }
        }

        let tid_slot = self.thread_id.clone();
        tid_slot.store(0, Ordering::SeqCst);
        // Key-ups that happened while stopped were never seen: start from a clean slate.
        KEYS.clear_held();
        HOOK_ACTIVE.store(true, Ordering::SeqCst);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);

        let handle = thread::Builder::new()
            .name("opengear-input-hook".to_string())
            .spawn(move || unsafe {
                // Create this thread's message queue BEFORE publishing its id, so that
                // PostThreadMessageW from stop()/update_rules() can never be lost.
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
                // Keep the handle so stop() can still clean the thread up.
                self.worker = Some(handle);
                let err = "Le hook clavier n'a pas confirmé son installation (délai dépassé)".to_string();
                self.last_start_failure = Some((Instant::now(), err.clone()));
                anyhow::bail!(err)
            }
        }
    }

    #[cfg(not(windows))]
    pub fn start(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("InputEngine requiert Windows");
    }

    /// Stop the hooks: cancel running macros, release every key OpenGear holds down, then ask the
    /// hook thread to quit. Never blocks forever: if the thread does not exit in time it is
    /// detached (its hook is already inert since `HOOK_ACTIVE` is false).
    #[cfg(windows)]
    pub fn stop(&mut self) {
        HOOK_ACTIVE.store(false, Ordering::SeqCst);
        release_all_injected_keys();
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
    pub fn stop(&mut self) {}
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
    fn snap_tap_pairs_match_physical_keys_whatever_the_layout() {
        // AZERTY preset "Q+D" and QWERTY preset "A+D" are the same physical keys.
        assert_eq!(scan_for_name("Q", 0x51), 0x1E);
        assert_eq!(scan_for_name("A", 0x41), 0x1E);
        assert_eq!(scan_for_name("d", 0x44), 0x20);
        assert_eq!(scan_for_name("Z", 0x5A), 0x11);
        assert_eq!(scan_for_name("W", 0x57), 0x11);
        let pair = SocdPairRuntime { vk_a: 0x51, vk_b: 0x44, scan_a: 0x1E, scan_b: 0x20 };
        // Physical left key reported with an unexpected VK (e.g. 0x41) is still "A side"
        assert_eq!(pair.side_of(0x41, 0x1E), Some(true));
        assert_eq!(pair.side_of(0x00, 0x20), Some(false));
        // VK fallback when no scan code
        assert_eq!(pair.side_of(0x51, 0), Some(true));
        // Unrelated key
        assert_eq!(pair.side_of(0x45, 0x12), None);
    }

    #[test]
    fn observed_scan_code_is_reused_with_its_extended_flag() {
        let k = KeyTracker::new();
        assert_eq!(k.observed_scan(0x51), None);
        // AZERTY: VK_Q is produced by the physical key at scan 0x1E
        k.note_scan(0x51, 0x1E, false);
        assert_eq!(k.observed_scan(0x51), Some((0x1E, false)));
        // A zero scan code never overwrites a real one
        k.note_scan(0x51, 0, false);
        assert_eq!(k.observed_scan(0x51), Some((0x1E, false)));
        k.note_scan(0x25, 0x4B, true); // left arrow (extended)
        assert_eq!(k.observed_scan(0x25), Some((0x4B, true)));
    }

    #[test]
    fn auto_repeat_is_detected_only_while_held() {
        let k = KeyTracker::new();
        assert!(!k.note_physical(0x41, true), "first press is not a repeat");
        assert!(k.note_physical(0x41, true), "second down while held is OS auto-repeat");
        assert!(k.note_physical(0x41, true));
        assert!(!k.note_physical(0x41, false));
        assert!(!k.note_physical(0x41, true), "new press after release");
    }

    #[test]
    fn clear_held_forgets_keys_released_while_stopped() {
        let k = KeyTracker::new();
        k.note_physical(0x20, true);
        k.clear_held();
        assert!(!k.note_physical(0x20, true));
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
        k.begin_remap(0x14, 0x7C); // CAPS -> F13
        assert_eq!(k.remap_target(0x14), Some(0x7C));
        // Even if the rules changed meanwhile, the release undoes what was pressed.
        assert_eq!(k.end_remap(0x14), Some(0x7C));
        assert_eq!(k.end_remap(0x14), None, "released only once");
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
        track_pressed(&mut pressed, 0xA2, true); // LCTRL down
        track_pressed(&mut pressed, 0x43, true); // C down
        track_pressed(&mut pressed, 0x43, true); // duplicate down
        track_pressed(&mut pressed, 0x43, false); // C up
        // Truncated recording: LCTRL never released -> must be released at the end.
        assert_eq!(pressed, vec![0xA2]);
    }

    #[test]
    fn delays_are_capped() {
        assert_eq!(capped_delay(50, MAX_MACRO_STEP_DELAY_MS), Duration::from_millis(50));
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
