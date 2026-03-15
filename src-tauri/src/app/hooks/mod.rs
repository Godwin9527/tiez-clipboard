use tauri::{AppHandle, Emitter, Manager};
use std::sync::atomic::Ordering;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT,
    WM_KEYDOWN, WM_SYSKEYDOWN, WM_KEYUP, WM_SYSKEYUP,
    WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEWHEEL
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN,
    RegisterHotKey, UnregisterHotKey, MOD_WIN, MOD_NOREPEAT
};

use crate::global_state::*;
use crate::app_state::SettingsState;
use crate::app::window_manager::{toggle_window, hide_window_cmd};
use crate::infrastructure::windows_ext::WindowExt;

// Store registered hotkey IDs for cleanup
static BLOCKED_HOTKEY_IDS: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());

// Cooldown flag: consume remaining modifier key-ups after quick paste confirm
// to prevent Windows input language switch (Ctrl+Shift) from stealing focus.
use std::sync::atomic::AtomicBool;
static QUICK_PASTE_CONFIRM_COOLDOWN: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn set_recording_mode(app_handle: AppHandle, enabled: bool) -> Result<(), String> {
    IS_RECORDING.store(enabled, Ordering::SeqCst);
    
    let mut ids = BLOCKED_HOTKEY_IDS.lock().unwrap();
    
    #[cfg(target_os = "windows")]
    if enabled {
        // Register ALL Win+ combinations to block system from handling them
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(hwnd_raw) = window.hwnd() {
                let hwnd = HWND(hwnd_raw.0);
                let mut id_counter = 0x1000i32;
                
                // Block Win + A-Z
                for vk in 0x41u32..=0x5Au32 {
                    unsafe {
                        if RegisterHotKey(Some(hwnd), id_counter, MOD_WIN | MOD_NOREPEAT, vk).is_ok() {
                            ids.push(id_counter);
                        }
                    }
                    id_counter += 1;
                }
                
                // Block Win + 0-9
                for vk in 0x30u32..=0x39u32 {
                    unsafe {
                        if RegisterHotKey(Some(hwnd), id_counter, MOD_WIN | MOD_NOREPEAT, vk).is_ok() {
                            ids.push(id_counter);
                        }
                    }
                    id_counter += 1;
                }
                
                // Block special keys
                let special_keys = [0x20u32, 0x0D, 0x09, 0x1B, 0x2C]; // Space, Enter, Tab, Esc, PrintScreen
                for vk in special_keys {
                    unsafe {
                        if RegisterHotKey(Some(hwnd), id_counter, MOD_WIN | MOD_NOREPEAT, vk).is_ok() {
                            ids.push(id_counter);
                        }
                    }
                    id_counter += 1;
                }
                println!("Recording mode ON: Blocked {} Win+ combinations", ids.len());
            }
        }
    } else {
        // Unregister all blocked hotkeys
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(hwnd_raw) = window.hwnd() {
                let hwnd = HWND(hwnd_raw.0);
                for id in ids.drain(..) {
                    unsafe {
                        let _ = UnregisterHotKey(Some(hwnd), id);
                    }
                }
                println!("Recording mode OFF: Released blocked hotkeys");
            }
        }
    }
    
    Ok(())
}

// Low-level Keyboard Hook Procedure
#[cfg(target_os = "windows")]
pub unsafe extern "system" fn keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    let msg = w_param.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    if n_code >= 0 && (is_down || is_up) {
        let kbd_struct = *(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd_struct.vkCode;

        // Handle Recording Mode - Black Hole Logic
        if IS_RECORDING.load(Ordering::SeqCst) {
            let ctrl_down = GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000 != 0;
            let shift_down = GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000 != 0;
            let alt_down = GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000 != 0;
            let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || 
                          (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);

            // ESC to cancel
            if vk == 0x1B && is_down {
                IS_RECORDING.store(false, Ordering::SeqCst);
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let _ = handle.emit("recording-cancelled", ());
                }
                return CallNextHookEx(None, n_code, w_param, l_param);
            }

            let is_win = vk == 0x5B || vk == 0x5C;
            let is_other_modifier = (vk >= 0x10 && vk <= 0x12) || (vk >= 0xA0 && vk <= 0xA5);
            
            if is_other_modifier {
                return CallNextHookEx(None, n_code, w_param, l_param);
            }

            if !is_win && is_down {
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let key_name = match vk {
                        0x20 => "Space".to_string(),
                        0x0D => "Enter".to_string(),
                        0x09 => "Tab".to_string(),
                        0x08 => "Backspace".to_string(),
                        0x2E => "Delete".to_string(),
                        0x2D => "Insert".to_string(),
                        0x21 => "PageUp".to_string(),
                        0x22 => "PageDown".to_string(),
                        0x23 => "End".to_string(),
                        0x24 => "Home".to_string(),
                        0x25 => "Left".to_string(),
                        0x26 => "Up".to_string(),
                        0x27 => "Right".to_string(),
                        0x28 => "Down".to_string(),
                        0xBB => "Plus".to_string(),
                        0xBC => "Comma".to_string(),
                        0xBD => "Minus".to_string(),
                        0xBE => "Period".to_string(),
                        0xBF => "/".to_string(),
                        0xC0 => "`".to_string(),
                        0xBA => ";".to_string(),
                        0xDB => "[".to_string(),
                        0xDC => "\\".to_string(),
                        0xDD => "]".to_string(),
                        0xDE => "'".to_string(),
                        k if k >= 0x70 && k <= 0x87 => format!("F{}", k - 0x6F),
                        k if (k >= 0x30 && k <= 0x39) || (k >= 0x41 && k <= 0x5A) => 
                            format!("{}", char::from_u32(k).unwrap()),
                        _ => format!("Key_{}", vk)
                    };

                    let final_hotkey = format!("{}{}{}{}{}", 
                        if ctrl_down { "Ctrl+" } else { "" },
                        if shift_down { "Shift+" } else { "" },
                        if alt_down { "Alt+" } else { "" },
                        if win_down { "Win+" } else { "" },
                        key_name
                    );
                    
                    println!("Recorded Hotkey: {}", final_hotkey);
                    let _ = handle.emit("hotkey-recorded", final_hotkey);
                    IS_RECORDING.store(false, Ordering::SeqCst);
                }
            }
            return LRESULT(1);
        }

        // 3. Global Paste Sound Trigger (Ctrl+V)
        {
             let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
             let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
             let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
             let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);

             if vk == 0x56 && ctrl_down && !alt_down && !shift_down && !win_down {
                  if is_down {
                       if let Some(handle) = GLOBAL_APP_HANDLE.get() {

                          let settings = handle.state::<SettingsState>();
                          if settings.sound_enabled.load(Ordering::Relaxed) {
                              std::thread::spawn(move || {
                                  let _ = handle.emit("play-sound", "paste");
                              });
                          }
                      }
                  }
             }
        }

        // 4. Scroll-to-top hotkey (handled via hook so other apps can use the key when window is hidden)
        if is_down {
            if let Ok(guard) = SCROLL_TOP_HOTKEY.lock() {
                if let Some(ref hk) = *guard {
                    if vk == hk.vk {
                        let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                        let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
                        let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);
                        if ctrl_down == hk.ctrl && shift_down == hk.shift && alt_down == hk.alt && win_down == hk.win {
                            // Only consume the key when the clipboard window is visible
                            if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                                if let Some(window) = handle.get_webview_window("main") {
                                    if let Ok(hwnd_raw) = window.hwnd() {
                                        let main_hwnd = HWND(hwnd_raw.0);
                                        let is_visible = WindowExt::is_window_visible(main_hwnd);
                                        let is_hidden_by_edge = IS_HIDDEN.load(Ordering::Relaxed);
                                        if is_visible && !is_hidden_by_edge {
                                            let _ = handle.emit("scroll-to-top", ());
                                            return LRESULT(1);
                                        }
                                    }
                                }
                            }
                            // Window not visible: pass through to other apps
                        }
                    }
                }
            }
        }

        // 5. Global Navigation Keys (Up/Down, Enter, Esc)
        if NAVIGATION_ENABLED.load(Ordering::SeqCst) && !IS_RECORDING.load(Ordering::SeqCst) {
             if IS_HIDDEN.load(Ordering::Relaxed) {
                 return CallNextHookEx(None, n_code, w_param, l_param);
             }
             // Only intercept navigation keys when clipboard window has focus
             if !IS_MAIN_WINDOW_FOCUSED.load(Ordering::Relaxed) {
                 return CallNextHookEx(None, n_code, w_param, l_param);
             }
             let allow_navigation = if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                 let settings = handle.state::<SettingsState>();
                 settings.arrow_key_selection.load(Ordering::Relaxed)
             } else {
                 true
             };

             if !allow_navigation {
                 return CallNextHookEx(None, n_code, w_param, l_param);
             }

             let is_navigation_key = vk == 0x26 || vk == 0x28 || vk == 0x0D || vk == 0x1B;
             let is_enter = vk == 0x0D;
             let is_escape = vk == 0x1B;
             
              if is_navigation_key && is_down {
                   if (is_enter || is_escape) && !NAVIGATION_MODE_ACTIVE.load(Ordering::Relaxed) {
                       return CallNextHookEx(None, n_code, w_param, l_param);
                   }
                   let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                   let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
                   let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);

                   if !ctrl_down && !alt_down && !win_down {
                       if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                           let action = match vk {
                               0x26 => "up",
                               0x28 => "down",
                               0x0D => "enter",
                               0x1B => "escape",
                               _ => "",
                           };
                           
                           if !action.is_empty() {
                               if vk == 0x26 || vk == 0x28 {
                                   NAVIGATION_MODE_ACTIVE.store(true, Ordering::Relaxed);
                               } else if vk == 0x1B {
                                   NAVIGATION_MODE_ACTIVE.store(false, Ordering::Relaxed);
                               }
                               if action == "escape" {
                                   let handle_clone = handle.clone();
                                   tauri::async_runtime::spawn(async move {
                                       let _ = handle_clone.emit("navigation-action", "escape");
                                       toggle_window(&handle_clone);
                                  });
                              } else {
                                  let _ = handle.emit("navigation-action", action);
                              }
                              return LRESULT(1);
                          }
                      }
                  }
             }
        }

        // Consume remaining modifier releases after quick paste confirm
        // to prevent Ctrl+Shift language switch from stealing focus
        if QUICK_PASTE_CONFIRM_COOLDOWN.load(Ordering::Relaxed) {
            let is_modifier = vk == 0x10 || vk == 0x11 || (vk >= 0xA0 && vk <= 0xA3);
            if is_up && is_modifier {
                QUICK_PASTE_CONFIRM_COOLDOWN.store(false, Ordering::Relaxed);
                return LRESULT(1);
            }
            if is_down {
                QUICK_PASTE_CONFIRM_COOLDOWN.store(false, Ordering::Relaxed);
            }
        }

        // Quick Paste Mode handling
        if QUICK_PASTE_MODE.load(Ordering::Relaxed) {
            // On ESC keydown: cancel quick paste
            if is_down && vk == 0x1B {
                QUICK_PASTE_MODE.store(false, Ordering::Relaxed);
                NAVIGATION_MODE_ACTIVE.store(false, Ordering::Relaxed);
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let handle_clone = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = handle_clone.emit("navigation-action", "escape");
                        toggle_window(&handle_clone);
                    });
                }
                return LRESULT(1);
            }

            // On Ctrl or Shift keyup: confirm paste immediately
            // Don't use GetAsyncKeyState here - it's unreliable in low-level hooks
            // (the key state may not be updated yet when the hook fires)
            let is_ctrl_or_shift = vk == 0x10 || vk == 0x11 ||
                (vk >= 0xA0 && vk <= 0xA3); // LShift, RShift, LCtrl, RCtrl
            if is_up && is_ctrl_or_shift {
                QUICK_PASTE_MODE.store(false, Ordering::Relaxed);
                NAVIGATION_MODE_ACTIVE.store(false, Ordering::Relaxed);
                NAVIGATION_ENABLED.store(false, Ordering::SeqCst);
                QUICK_PASTE_CONFIRM_COOLDOWN.store(true, Ordering::Relaxed);
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let _ = handle.emit("quick-paste-confirm", ());
                }
                return LRESULT(1);
            }

            // In quick-paste mode, arrow keys navigate (if nav mode allows)
            let nav_mode = QUICK_PASTE_NAV_MODE.load(Ordering::Relaxed);
            let arrow_allowed = nav_mode == 2 || nav_mode == 3; // arrow or both
            if is_down && (vk == 0x26 || vk == 0x28) && arrow_allowed {
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let action = if vk == 0x26 { "up" } else { "down" };
                    let _ = handle.emit("navigation-action", action);
                    return LRESULT(1);
                }
            }
        } else {
            // Not in quick-paste mode: detect Ctrl+Shift+Arrow to activate
            let nav_mode = QUICK_PASTE_NAV_MODE.load(Ordering::Relaxed);
            if nav_mode > 0 && is_down && (vk == 0x26 || vk == 0x28) {
                let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
                let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);

                let arrow_allowed = nav_mode == 2 || nav_mode == 3;
                if ctrl_down && shift_down && !alt_down && !win_down && arrow_allowed {
                    // Activate quick-paste mode
                    QUICK_PASTE_MODE.store(true, Ordering::Relaxed);
                    NAVIGATION_ENABLED.store(true, Ordering::SeqCst);
                    NAVIGATION_MODE_ACTIVE.store(true, Ordering::Relaxed);
                    if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                        let handle_clone = handle.clone();
                        let action = if vk == 0x26 { "up" } else { "down" };
                        tauri::async_runtime::spawn(async move {
                            // Only show window if not already visible
                            if let Some(window) = handle_clone.get_webview_window("main") {
                                let is_visible = window.is_visible().unwrap_or(false);
                                let is_hidden_by_edge = IS_HIDDEN.load(Ordering::Relaxed);
                                if !is_visible || is_hidden_by_edge {
                                    toggle_window(&handle_clone);
                                }
                            }
                            let _ = handle_clone.emit("quick-paste-activated", ());
                            let _ = handle_clone.emit("navigation-action", action);
                        });
                    }
                    return LRESULT(1);
                }
            }
        }

    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

// Low-level Mouse Hook Procedure
#[cfg(target_os = "windows")]
pub unsafe extern "system" fn mouse_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        let msg = w_param.0 as u32;
        if msg == WM_MBUTTONDOWN || msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || 
           msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP || 
           msg == windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP {
            
            // Track mouse state globally
            if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
                IS_MOUSE_BUTTON_DOWN.store(true, Ordering::SeqCst);
            } else if msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP || msg == windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP {
                IS_MOUSE_BUTTON_DOWN.store(false, Ordering::SeqCst);
                return CallNextHookEx(None, n_code, w_param, l_param); // Return early for up events
            }

            // Handle Recording Mode
            if IS_RECORDING.load(Ordering::SeqCst) && msg == WM_MBUTTONDOWN {
                IS_RECORDING.store(false, Ordering::SeqCst);
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let _ = handle.emit("hotkey-recorded", "MouseMiddle");
                }
                return LRESULT(1);
            }

            // Click Elsewhere to Hide Logic
            if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    if let Some(window) = handle.get_webview_window("main") {
                        if !IGNORE_BLUR.load(Ordering::Relaxed) {
                            let mouse_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
                            let point = mouse_struct.pt;
                            
                                    if let Ok(hwnd_raw) = window.hwnd() {
                                        let main_hwnd = HWND(hwnd_raw.0);
                                        if !WindowExt::is_window_visible(main_hwnd) {
                                            return CallNextHookEx(None, n_code, w_param, l_param);
                                        }
                                        let mut rect = RECT::default();
                                        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(main_hwnd, &mut rect);

                                        // Boundary check: Is point outside the rect? (with 5px margin of safety)
                                        let margin = 5;
                                        let is_outside = point.x < rect.left - margin || point.x > rect.right + margin ||
                                            point.y < rect.top - margin || point.y > rect.bottom + margin;

                                        if is_outside {
                                            // Status check before hiding
                                            if !WindowExt::is_window_visible(main_hwnd) {
                                                return CallNextHookEx(None, n_code, w_param, l_param);
                                            }

                                            if WINDOW_PINNED.load(Ordering::Relaxed) {
                                                // Pinned: Just reset focusable state to ensure we don't retain focus
                                                let _ = window.set_focusable(false);
                                            } else {
                                                let _ = hide_window_cmd(handle.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
            }

            // Handle configured middle mouse hotkey
            if msg == WM_MBUTTONDOWN {
                let current = HOTKEY_STRING.lock().unwrap().to_lowercase();
                if current == "mousemiddle" || current == "mbutton" {
                    if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                        toggle_window(&handle);
                    }
                    return LRESULT(1);
                }
            }
        }

        // Mouse wheel in quick-paste mode or Ctrl+Shift activation
        if msg == WM_MOUSEWHEEL {
            let nav_mode = QUICK_PASTE_NAV_MODE.load(Ordering::Relaxed);
            let wheel_allowed = nav_mode == 1 || nav_mode == 3; // wheel or both

            if QUICK_PASTE_MODE.load(Ordering::Relaxed) && wheel_allowed {
                let mouse_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
                let hi_word = ((mouse_struct.mouseData >> 16) & 0xFFFF) as i16;
                let action = if hi_word > 0 { "up" } else { "down" };
                if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                    let _ = handle.emit("navigation-action", action);
                }
                return LRESULT(1);
            } else if !QUICK_PASTE_MODE.load(Ordering::Relaxed) && nav_mode > 0 && wheel_allowed {
                // Detect Ctrl+Shift+Wheel to activate quick-paste mode
                let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
                let win_down = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000 != 0) || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000 != 0);

                if ctrl_down && shift_down && !alt_down && !win_down {
                    let mouse_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
                    let hi_word = ((mouse_struct.mouseData >> 16) & 0xFFFF) as i16;
                    let action = if hi_word > 0 { "up" } else { "down" };

                    QUICK_PASTE_MODE.store(true, Ordering::Relaxed);
                    NAVIGATION_ENABLED.store(true, Ordering::SeqCst);
                    NAVIGATION_MODE_ACTIVE.store(true, Ordering::Relaxed);
                    if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                        let handle_clone = handle.clone();
                        let action_str = action.to_string();
                        tauri::async_runtime::spawn(async move {
                            // Only show window if not already visible
                            if let Some(window) = handle_clone.get_webview_window("main") {
                                let is_visible = window.is_visible().unwrap_or(false);
                                let is_hidden_by_edge = IS_HIDDEN.load(Ordering::Relaxed);
                                if !is_visible || is_hidden_by_edge {
                                    toggle_window(&handle_clone);
                                }
                            }
                            let _ = handle_clone.emit("quick-paste-activated", ());
                            let _ = handle_clone.emit("navigation-action", action_str);
                        });
                    }
                    return LRESULT(1);
                }
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

pub fn parse_hotkey_for_hook(hotkey: &str) -> Option<HookHotkey> {
    let parts: Vec<&str> = hotkey.split('+').collect();
    let mut vk = 0u32;
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut win = false;

    for part in parts {
        let part_upper = part.trim().to_uppercase();
        match part_upper.as_str() {
            "CTRL" | "CONTROL" => ctrl = true,
            "SHIFT" => shift = true,
            "ALT" | "MENU" => alt = true,
            "SUPER" | "WIN" | "COMMAND" | "META" => win = true,
            "SPACE" => vk = 0x20,
            "ENTER" | "RETURN" => vk = 0x0D,
            "TAB" => vk = 0x09,
            "BACKSPACE" => vk = 0x08,
            "DELETE" => vk = 0x2E,
            "INSERT" => vk = 0x2D,
            "PAGEUP" => vk = 0x21,
            "PAGEDOWN" => vk = 0x22,
            "END" => vk = 0x23,
            "HOME" => vk = 0x24,
            "LEFT" => vk = 0x25,
            "UP" => vk = 0x26,
            "RIGHT" => vk = 0x27,
            "DOWN" => vk = 0x28,
            "PLUS" | "=" => vk = 0xBB,
            "COMMA" | "," => vk = 0xBC,
            "MINUS" | "-" => vk = 0xBD,
            "PERIOD" | "." => vk = 0xBE,
            "/" | "SLASH" => vk = 0xBF,
            "`" | "TILDE" | "GRAVE" => vk = 0xC0,
            ";" | "SEMICOLON" => vk = 0xBA,
            "[" | "LBRACKET" => vk = 0xDB,
            "\\" | "BACKSLASH" => vk = 0xDC,
            "]" | "RBRACKET" => vk = 0xDD,
            "'" | "QUOTE" => vk = 0xDE,
            key if key.starts_with('F') && key.len() > 1 => {
                if let Ok(num) = key[1..].parse::<u32>() {
                    if (1..=24).contains(&num) {
                        vk = 0x6F + num;
                    }
                }
            }
            key => {
                if key.len() == 1 {
                    vk = key.chars().next().unwrap() as u32;
                }
            }
        }
    }
    
    if vk != 0 {
        Some(HookHotkey { vk, ctrl, shift, alt, win })
    } else {
        None
    }
}

pub fn is_win_v_hotkey(hotkey: &str) -> bool {
    let parts: Vec<String> = hotkey
        .split('+')
        .map(|p| p.trim().to_uppercase())
        .filter(|p| !p.is_empty())
        .collect();

    if parts.is_empty() {
        return false;
    }

    let mut has_win = false;
    let mut has_v = false;

    for part in &parts {
        match part.as_str() {
            "WIN" | "SUPER" | "COMMAND" | "META" => has_win = true,
            "V" => has_v = true,
            _ => return false,
        }
    }

    has_win && has_v
}
