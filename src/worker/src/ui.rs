use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use winapi::shared::minwindef::{DWORD, HINSTANCE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HMENU, HWND};
use winapi::um::commctrl::{
    InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETMARQUEE, PBS_MARQUEE,
    PROGRESS_CLASS,
};
use winapi::um::handleapi::CloseHandle;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
use winapi::um::wingdi::{GetStockObject, DEFAULT_GUI_FONT};
use winapi::um::winnt::PROCESS_TERMINATE;
use winapi::um::winuser::*;

const ID_STATUS: usize = 1001;
const ID_BAR: usize = 1002;
const ID_CANCEL: usize = 1003;
const WINDOW_CLASS: &str = "R1fcConvertProgress";

pub struct UiState {
    pub status: String,
    pub done: bool,
    pub success: bool,
    pub cancelled: bool,
    pub detail: String,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            status: "Starting...".to_string(),
            done: false,
            success: false,
            cancelled: false,
            detail: String::new(),
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

struct AppCtx {
    state: Arc<Mutex<UiState>>,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    status: HWND,
    bar: HWND,
    cancel_button: HWND,
    done_handled: bool,
    message_shown: bool,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn set_text(hwnd: HWND, value: &str) {
    if hwnd.is_null() {
        return;
    }
    let text = wide(value);
    SetWindowTextW(hwnd, text.as_ptr());
}

unsafe fn get_ctx(hwnd: HWND) -> *mut AppCtx {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppCtx
}

fn kill_pid(pid: &AtomicU32) {
    let pid = pid.load(Ordering::SeqCst);
    if pid == 0 {
        return;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            let ctx_ptr = get_ctx(hwnd);
            if ctx_ptr.is_null() {
                return 0;
            }
            let ctx = &mut *ctx_ptr;

            if wparam == 1 {
                let snapshot = {
                    let state = match ctx.state.lock() {
                        Ok(state) => state,
                        Err(_) => return 0,
                    };
                    (
                        state.status.clone(),
                        state.done,
                        state.success,
                        state.cancelled,
                        state.detail.clone(),
                    )
                };

                set_text(ctx.status, &snapshot.0);

                if snapshot.1 && !ctx.done_handled {
                    ctx.done_handled = true;
                    KillTimer(hwnd, 1);
                    EnableWindow(ctx.cancel_button, 0);
                    SendMessageW(ctx.bar, PBM_SETMARQUEE, 0, 0);
                    let delay = if snapshot.2 || snapshot.3 { 1200 } else { 400 };
                    SetTimer(hwnd, 2, delay, None);
                }
            } else if wparam == 2 {
                let show_error = {
                    let state = match ctx.state.lock() {
                        Ok(state) => state,
                        Err(_) => return 0,
                    };
                    !state.success && !state.cancelled && !ctx.message_shown
                };

                if show_error {
                    ctx.message_shown = true;
                    let detail = match ctx.state.lock() {
                        Ok(state) => state.detail.clone(),
                        Err(_) => "The conversion failed. See the log file for details.".to_string(),
                    };
                    let text = wide(&detail);
                    let title = wide("R1fc Convert");
                    MessageBoxW(
                        hwnd,
                        text.as_ptr(),
                        title.as_ptr(),
                        MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
                    );
                }

                DestroyWindow(hwnd);
            }

            0
        }
        WM_COMMAND => {
            if (wparam & 0xffff) == ID_CANCEL {
                let ctx_ptr = get_ctx(hwnd);
                if !ctx_ptr.is_null() {
                    let ctx = &mut *ctx_ptr;
                    ctx.cancel.store(true, Ordering::SeqCst);
                    kill_pid(&ctx.pid);
                    EnableWindow(ctx.cancel_button, 0);
                    set_text(ctx.status, "Cancelling...");
                    if let Ok(mut state) = ctx.state.lock() {
                        state.status = "Cancelling...".to_string();
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            let ctx_ptr = get_ctx(hwnd);
            if !ctx_ptr.is_null() {
                let ctx = &mut *ctx_ptr;
                ctx.cancel.store(true, Ordering::SeqCst);
                kill_pid(&ctx.pid);
            }
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            let ctx_ptr = get_ctx(hwnd);
            if !ctx_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx_ptr));
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

pub fn run_with_progress<F>(
    state: Arc<Mutex<UiState>>,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    work: F,
) where
    F: FnOnce() + Send + 'static,
{
    unsafe {
        let instance = GetModuleHandleW(null_mut());
        let class_name = wide(WINDOW_CLASS);

        let mut window_class: WNDCLASSW = zeroed();
        window_class.style = CS_HREDRAW | CS_VREDRAW;
        window_class.lpfnWndProc = Some(wndproc);
        window_class.hInstance = instance;
        window_class.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        window_class.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        window_class.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
        window_class.lpszClassName = class_name.as_ptr();
        RegisterClassW(&window_class);

        let mut controls: INITCOMMONCONTROLSEX = zeroed();
        controls.dwSize = size_of::<INITCOMMONCONTROLSEX>() as DWORD;
        controls.dwICC = ICC_PROGRESS_CLASS;
        InitCommonControlsEx(&controls);

        let title = wide("R1fc Convert");
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            200,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );

        if hwnd.is_null() {
            work();
            return;
        }

        let ctx = Box::new(AppCtx {
            state,
            cancel,
            pid,
            status: null_mut(),
            bar: null_mut(),
            cancel_button: null_mut(),
            done_handled: false,
            message_shown: false,
        });
        let ctx_ptr = Box::into_raw(ctx);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

        let status_text = wide("Starting...");
        let status = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            status_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT | SS_NOPREFIX,
            16,
            16,
            448,
            44,
            hwnd,
            ID_STATUS as usize as HMENU,
            instance,
            null_mut(),
        );

        let progress_class = wide(PROGRESS_CLASS);
        let bar = CreateWindowExW(
            0,
            progress_class.as_ptr(),
            null_mut(),
            WS_CHILD | WS_VISIBLE | PBS_MARQUEE,
            16,
            66,
            448,
            18,
            hwnd,
            ID_BAR as usize as HMENU,
            instance,
            null_mut(),
        );

        let cancel_text = wide("Cancel");
        let cancel_button = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            cancel_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            384,
            104,
            80,
            28,
            hwnd,
            ID_CANCEL as usize as HMENU,
            instance,
            null_mut(),
        );

        let font = GetStockObject(DEFAULT_GUI_FONT as i32);
        for control in [status, bar, cancel_button] {
            if !control.is_null() {
                SendMessageW(control, WM_SETFONT, font as WPARAM, 1);
            }
        }

        (*ctx_ptr).status = status;
        (*ctx_ptr).bar = bar;
        (*ctx_ptr).cancel_button = cancel_button;

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        SetWindowPos(
            hwnd,
            null_mut(),
            (screen_width - 480) / 2,
            (screen_height - 200) / 2,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        SetForegroundWindow(hwnd);
        FlashWindow(hwnd, 1);
        SetTimer(hwnd, 1, 100, None);

        std::thread::spawn(work);

        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

pub fn message_box_error(message: &str) {
    unsafe {
        let text = wide(message);
        let title = wide("R1fc Convert");
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

pub fn _unused(_: *mut c_void, _: HINSTANCE) {}
