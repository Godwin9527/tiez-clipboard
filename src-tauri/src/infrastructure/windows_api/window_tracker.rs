use std::path::Path;
use std::sync::atomic::Ordering;
use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
};
use windows::Win32::Foundation::HWND;
use crate::global_state::LAST_ACTIVE_HWND;

pub fn get_active_app_info() -> (String, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ("Unknown".to_string(), "Unknown".to_string());
        }

        // Get Window Title
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else {
            "Unknown".to_string()
        };

        // Get Process Name
        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));

        let process_handle = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            process_id,
        );

        let app_name = if let Ok(handle) = process_handle {
            let mut path_buf = [0u16; 1024];
            let path_len = GetModuleFileNameExW(Some(handle), None, &mut path_buf);
            if path_len > 0 {
                let path_str = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                Path::new(&path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        (app_name, title)
    }
}

pub fn start_window_tracking(_app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        unsafe {
            // Register hook to monitor window foreground changes
            let _hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(event_hook_callback),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );

            // Keep the thread alive and processing messages
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while windows::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
    });
}

fn is_own_process_window(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    process_id != 0 && process_id == unsafe { GetCurrentProcessId() }
}

pub fn is_window_visible(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    unsafe { IsWindowVisible(hwnd).as_bool() }
}

fn is_system_focus_window(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return true;
    }

    let mut class_name = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut class_name) };
    let class_str = if len > 0 {
        String::from_utf16_lossy(&class_name[..len as usize])
    } else {
        String::new()
    };

    matches!(
        class_str.as_str(),
        // Taskbar / tray / shell surfaces (these should NOT receive paste)
        "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "TrayNotifyWnd"
            | "NotifyIconOverflowWindow"
            | "ReBarWindow32"
            | "MSTaskSwWClass"
            // Note: Progman and WorkerW (desktop windows) are intentionally NOT filtered
            // because users may need to paste when renaming files on the desktop
            | "Button"
            // System UI overlays that should NOT receive paste
            | "ImmersiveLauncher"
            | "ShellExperienceHost"
            | "TaskSwitcherWnd"
            | "MultitaskingViewFrame"
            // Note: Windows.UI.Core.CoreWindow, SearchUI, Cortana, XamlExplorerHostIslandWindow,
            // and ApplicationFrameWindow are intentionally NOT filtered because users may need
            // to paste in Windows search box and other UWP app input fields
    )
}

unsafe extern "system" fn event_hook_callback(
    _h_win_event_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _dw_event_thread: u32,
    _dwms_event_time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND && !hwnd.0.is_null() {
        // Skip hidden windows
        if !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return;
        }

        // Skip our own app windows
        if is_own_process_window(hwnd) {
            return;
        }

        // Skip system/shell windows that shouldn't receive paste
        if is_system_focus_window(hwnd) {
            return;
        }

        // Only store valid user windows (this is the "Save" part)
        LAST_ACTIVE_HWND.store(hwnd.0 as usize, Ordering::SeqCst);
        // println!("[DEBUG] Hook captured last focus HWND: {}", hwnd.0);
    }
}
