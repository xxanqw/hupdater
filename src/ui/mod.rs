use std::sync::Arc;
use windows::Win32::Foundation::{BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM, FALSE};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject, GetSysColorBrush, InvalidateRect,
    HBRUSH, HDC, HFONT, HPEN, LOGFONTW, MoveToEx, LineTo, PAINTSTRUCT, PS_SOLID, Rectangle, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    GET_STOCK_OBJECT_FLAGS,
    COLOR_BTNFACE, COLOR_WINDOW, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, LoadCursorW, LoadImageW, PostQuitMessage, RegisterClassExW,
    SendMessageW, SetWindowLongPtrW, ShowWindow, SystemParametersInfoW, TranslateMessage,
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND,
    MSG, WNDCLASSEXW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    WS_OVERLAPPED, WS_CAPTION, WS_SYSMENU, WS_MINIMIZEBOX, WS_CHILD, WS_VISIBLE, WINDOW_EX_STYLE, WINDOW_STYLE,
    GWLP_USERDATA, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_PAINT, WM_SETTINGCHANGE, WM_SIZE, WM_THEMECHANGED,
    WM_SETFONT,
    IDC_ARROW, IMAGE_ICON, LR_DEFAULTSIZE,
    BN_CLICKED, CB_ADDSTRING, CBN_SELCHANGE, CB_SETCURSEL, HICON, SW_SHOW,
};
use windows::core::PCWSTR;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;
use crate::to_wide_null;

pub mod events;

use events::{UiEvent, WM_UI_EVENT};

const DWMWCP_ROUND: i32 = 2;

const CBS_DROPDOWNLIST: WINDOW_STYLE = WINDOW_STYLE(0x0003);
const WS_TABSTOP: WINDOW_STYLE = WINDOW_STYLE(0x00010000);
#[repr(C)]
#[allow(non_snake_case)]
struct INITCOMMONCONTROLSEX {
    dwSize: u32,
    dwICC: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(picce: *mut INITCOMMONCONTROLSEX) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn UpdateWindow(hWnd: HWND) -> i32;
    fn EnableWindow(hWnd: HWND, bEnable: i32) -> i32;
    fn SetWindowTextW(hWnd: HWND, lpString: PCWSTR) -> i32;
}

pub struct AppCallbacks {
    pub on_trigger_update: Arc<dyn Fn() + Send + Sync>,
    pub on_enable_interceptor: Arc<dyn Fn() + Send + Sync>,
    pub on_restore_interceptor: Arc<dyn Fn() + Send + Sync>,
    pub on_uninstall_updater: Arc<dyn Fn() + Send + Sync>,
    pub on_save_settings: Arc<dyn Fn(bool) + Send + Sync>,
}

pub struct UiState {
    pub install_status: String,
    pub install_scope: String,
    pub task_info: String,
    pub is_busy: bool,
    pub use_winget: bool,
    pub interceptor_enabled: bool,
    pub controls_enabled: bool,
    pub helium_installed: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            install_status: "Installed (Registry & Winget)".to_string(),
            install_scope: "user".to_string(),
            task_info: "Ready".to_string(),
            is_busy: false,
            use_winget: false,
            interceptor_enabled: false,
            controls_enabled: false,
            helium_installed: false,
        }
    }
}

struct AppData {
    callbacks: AppCallbacks,
    state: UiState,
    dark_mode: bool,
    ui_font: HFONT,
    title_font: HFONT,
    background_brush: HBRUSH,
    surface_brush: HBRUSH,
    border_pen: HPEN,
    divider_pen: HPEN,
    background_color: COLORREF,
    surface_color: COLORREF,
    text_color: COLORREF,
    border_color: COLORREF,
    divider_color: COLORREF,
    hwnd_title: HWND,
    hwnd_subtitle: HWND,
    hwnd_helium_title: HWND,
    hwnd_group_helium: HWND,
    hwnd_status_label: HWND,
    hwnd_version_label: HWND,
    hwnd_check_install: HWND,
    hwnd_config_title: HWND,
    hwnd_group_config: HWND,
    hwnd_source_label: HWND,
    hwnd_combo_source: HWND,
    hwnd_interceptor_label: HWND,
    hwnd_enable: HWND,
    hwnd_restore: HWND,
    hwnd_help: HWND,
    hwnd_uninstall_label: HWND,
    hwnd_uninstall: HWND,
}

impl Drop for AppData {
    fn drop(&mut self) {
        unsafe {
            if self.ui_font.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.ui_font.0));
            }
            if self.title_font.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.title_font.0));
            }
            if self.background_brush.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.background_brush.0));
            }
            if self.surface_brush.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.surface_brush.0));
            }
            if self.border_pen.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.border_pen.0));
            }
            if self.divider_pen.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(self.divider_pen.0));
            }
        }
    }
}

const CLIENT_WIDTH: i32 = 480;
const CLIENT_HEIGHT: i32 = 340;

fn scale_for_dpi(value: i32, dpi: i32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

fn section_rects_for_dpi(dpi: i32) -> [RECT; 2] {
    let scale = |value: i32| scale_for_dpi(value, dpi);
    let margin = scale(12);
    let content_width = scale(CLIENT_WIDTH - 24);
    let title_y = scale(10);
    let title_h = scale(30);
    let subtitle_y = title_y + title_h + scale(2);
    let subtitle_h = scale(22);
    let section_title_h = scale(20);
    let frame_gap = scale(4);
    let helium_title_y = subtitle_y + subtitle_h + scale(12);
    let helium_y = helium_title_y + section_title_h + frame_gap;
    let helium_h = scale(76);
    let config_title_y = helium_y + helium_h + scale(12);
    let config_y = config_title_y + section_title_h + frame_gap;
    let config_h = scale(122);

    [
        RECT { left: margin, top: helium_y, right: margin + content_width, bottom: helium_y + helium_h },
        RECT { left: margin, top: config_y, right: margin + content_width, bottom: config_y + config_h },
    ]
}

fn section_dividers_for_dpi(dpi: i32) -> [(i32, i32, i32); 2] {
    let scale = |value: i32| scale_for_dpi(value, dpi);
    let margin = scale(12);
    let content_width = scale(CLIENT_WIDTH - 24);
    let title_y = scale(10);
    let title_h = scale(30);
    let subtitle_y = title_y + title_h + scale(2);
    let subtitle_h = scale(22);
    let section_title_h = scale(20);
    let helium_title_y = subtitle_y + subtitle_h + scale(12);
    let config_title_y = helium_title_y + section_title_h + scale(4) + scale(76) + scale(12);
    let divider_gap = scale(8);
    let divider_end = margin + content_width;

    [
        (margin + scale(52) + divider_gap, helium_title_y + section_title_h / 2, divider_end),
        (margin + scale(92) + divider_gap, config_title_y + section_title_h / 2, divider_end),
    ]
}

fn should_use_system_dark_mode() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .ok()
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme").ok())
        .map(|value| value == 0)
        .unwrap_or(false)
}

unsafe fn get_window_brush(data: &AppData) -> HBRUSH {
    if data.dark_mode && data.background_brush.0 != 0 {
        data.background_brush
    } else {
        unsafe { GetSysColorBrush(COLOR_BTNFACE) }
    }
}

unsafe fn get_surface_brush(data: &AppData) -> HBRUSH {
    if data.dark_mode && data.surface_brush.0 != 0 {
        data.surface_brush
    } else {
        unsafe { GetSysColorBrush(COLOR_WINDOW) }
    }
}

unsafe fn recreate_theme_resources(data: &mut AppData) {
    if data.background_brush.0 != 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(data.background_brush.0)) };
        data.background_brush = HBRUSH(0);
    }
    if data.surface_brush.0 != 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(data.surface_brush.0)) };
        data.surface_brush = HBRUSH(0);
    }
    if data.border_pen.0 != 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(data.border_pen.0)) };
        data.border_pen = HPEN(0);
    }
    if data.divider_pen.0 != 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(data.divider_pen.0)) };
        data.divider_pen = HPEN(0);
    }

    if data.dark_mode {
        data.background_color = rgb(32, 32, 32);
        data.surface_color = rgb(43, 43, 43);
        data.text_color = rgb(243, 243, 243);
        data.border_color = rgb(78, 78, 78);
        data.divider_color = rgb(96, 96, 96);
        data.background_brush = unsafe { CreateSolidBrush(data.background_color) };
        data.surface_brush = unsafe { CreateSolidBrush(data.surface_color) };
        data.border_pen = unsafe { CreatePen(PS_SOLID, 1, data.border_color) };
        data.divider_pen = unsafe { CreatePen(PS_SOLID, 1, data.divider_color) };
    } else {
        data.background_color = COLORREF(0);
        data.surface_color = COLORREF(0);
        data.text_color = COLORREF(0);
        data.border_color = rgb(210, 210, 210);
        data.divider_color = rgb(198, 198, 198);
        data.border_pen = unsafe { CreatePen(PS_SOLID, 1, data.border_color) };
        data.divider_pen = unsafe { CreatePen(PS_SOLID, 1, data.divider_color) };
    }
}

unsafe fn apply_control_theme(hwnd: HWND, theme_name: &str) {
    let theme = to_wide_null(theme_name);
    let _ = unsafe { SetWindowTheme(hwnd, PCWSTR(theme.as_ptr()), PCWSTR::null()) };
}

unsafe fn apply_window_theme(hwnd: HWND, data: &AppData) {
    let immersive = if data.dark_mode { 1i32 } else { 0i32 };
    let _ = unsafe { DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &immersive as *const _ as *const _,
        std::mem::size_of_val(&immersive) as u32,
    ) };

    for control in [
        data.hwnd_check_install,
        data.hwnd_enable,
        data.hwnd_restore,
        data.hwnd_help,
        data.hwnd_uninstall,
    ] {
        if control.0 != 0 {
            let theme = if data.dark_mode { "DarkMode_Explorer" } else { "Explorer" };
            unsafe { apply_control_theme(control, theme) };
        }
    }

    if data.hwnd_combo_source.0 != 0 {
        let theme = if data.dark_mode { "DarkMode_CFD" } else { "CFD" };
        unsafe { apply_control_theme(data.hwnd_combo_source, theme) };
    }

}

unsafe fn refresh_theme(hwnd: HWND, data: &mut AppData) {
    data.dark_mode = should_use_system_dark_mode();
    unsafe { recreate_theme_resources(data) };
    unsafe { apply_window_theme(hwnd, data) };

    for control in [
        hwnd,
        data.hwnd_title,
        data.hwnd_subtitle,
        data.hwnd_helium_title,
        data.hwnd_group_helium,
        data.hwnd_status_label,
        data.hwnd_version_label,
        data.hwnd_check_install,
        data.hwnd_config_title,
        data.hwnd_group_config,
        data.hwnd_source_label,
        data.hwnd_combo_source,
        data.hwnd_interceptor_label,
        data.hwnd_enable,
        data.hwnd_restore,
        data.hwnd_help,
        data.hwnd_uninstall_label,
        data.hwnd_uninstall,
    ] {
        if control.0 != 0 {
            let _ = unsafe { InvalidateRect(control, None, BOOL(1)) };
        }
    }
}

pub fn run_app(callbacks: AppCallbacks, initial_state: UiState) -> anyhow::Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;

        let class_name: Vec<u16> = "HUpdaterWindowClass\0".encode_utf16().collect();
        let title: Vec<u16> = "Helium Browser Autoupdater\0".encode_utf16().collect();
        let system_dpi = GetDpiForSystem() as i32;
        let scale = |value: i32| scale_for_dpi(value, system_dpi);

        let h_icon = LoadImageW(
            hinstance,
            PCWSTR(2 as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )?;

        let hcursor = LoadCursorW(None, IDC_ARROW)?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE(hinstance.0),
            hIcon: HICON(h_icon.0),
            hCursor: hcursor,
            hbrBackground: HBRUSH(0),
            lpszMenuName: PCWSTR(std::ptr::null()),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hIconSm: HICON(h_icon.0),
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            return Err(anyhow::anyhow!("RegisterClassExW failed"));
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: scale(CLIENT_WIDTH),
            bottom: scale(CLIENT_HEIGHT),
        };
        let _ = AdjustWindowRectEx(&mut rect, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, FALSE, WINDOW_EX_STYLE(0));

        let app_data = Box::into_raw(Box::new(AppData {
            callbacks,
            state: initial_state,
            dark_mode: false,
            ui_font: HFONT(0),
            title_font: HFONT(0),
            background_brush: HBRUSH(0),
            surface_brush: HBRUSH(0),
            border_pen: HPEN(0),
            divider_pen: HPEN(0),
            background_color: COLORREF(0),
            surface_color: COLORREF(0),
            text_color: COLORREF(0),
            border_color: COLORREF(0),
            divider_color: COLORREF(0),
            hwnd_title: HWND(0),
            hwnd_subtitle: HWND(0),
            hwnd_helium_title: HWND(0),
            hwnd_group_helium: HWND(0),
            hwnd_status_label: HWND(0),
            hwnd_version_label: HWND(0),
            hwnd_check_install: HWND(0),
            hwnd_config_title: HWND(0),
            hwnd_group_config: HWND(0),
            hwnd_source_label: HWND(0),
            hwnd_combo_source: HWND(0),
            hwnd_interceptor_label: HWND(0),
            hwnd_enable: HWND(0),
            hwnd_restore: HWND(0),
            hwnd_help: HWND(0),
            hwnd_uninstall_label: HWND(0),
            hwnd_uninstall: HWND(0),
        }));

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            hinstance,
            Some(app_data as *const _),
        );

        if hwnd.0 == 0 {
            return Err(anyhow::anyhow!("CreateWindowExW failed"));
        }

        events::set_ui_hwnd(hwnd);

        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        // Init common controls before creating other native controls
        let mut icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: 0x00000004 };
        InitCommonControlsEx(&mut icc);

        refresh_theme(hwnd, &mut *app_data);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = Box::from_raw(app_data);
        Ok(())
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            let app_data = (*cs).lpCreateParams as *mut AppData;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, app_data as isize);
            let hinstance = HINSTANCE((*cs).hInstance.0);
            create_controls(hwnd, &mut *app_data, hinstance);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                refresh_theme(hwnd, &mut *app_data);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = FillRect(HDC(wparam.0 as isize), &rect, get_window_brush(&*app_data));
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &*app_data;
                let dpi = match GetDpiForWindow(hwnd) {
                    0 => 96,
                    value => value as i32,
                };
                let border_pen = if data.border_pen.0 != 0 {
                    data.border_pen
                } else {
                    HPEN(GetStockObject(GET_STOCK_OBJECT_FLAGS(8)).0)
                };
                let divider_pen = if data.divider_pen.0 != 0 {
                    data.divider_pen
                } else {
                    border_pen
                };
                let old_pen = SelectObject(hdc, HGDIOBJ(border_pen.0));
                let old_brush = SelectObject(hdc, HGDIOBJ(GetStockObject(GET_STOCK_OBJECT_FLAGS(5)).0));
                for rect in section_rects_for_dpi(dpi) {
                    Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
                }
                let _ = SelectObject(hdc, HGDIOBJ(divider_pen.0));
                for (x1, y, x2) in section_dividers_for_dpi(dpi) {
                    let _ = MoveToEx(hdc, x1, y, None);
                    let _ = LineTo(hdc, x2, y);
                }
                let _ = SelectObject(hdc, old_brush);
                let _ = SelectObject(hdc, old_pen);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SIZE => {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORSTATIC => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &*app_data;
                if data.dark_mode {
                    let hdc = HDC(wparam.0 as isize);
                    SetTextColor(hdc, data.text_color);
                    SetBkMode(hdc, TRANSPARENT);
                    return LRESULT(get_window_brush(data).0 as isize);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORBTN => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &*app_data;
                if data.dark_mode {
                    let hdc = HDC(wparam.0 as isize);
                    SetTextColor(hdc, data.text_color);
                    SetBkMode(hdc, TRANSPARENT);
                    return LRESULT(get_window_brush(data).0 as isize);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &*app_data;
                if data.dark_mode {
                    let hdc = HDC(wparam.0 as isize);
                    SetTextColor(hdc, data.text_color);
                    SetBkColor(hdc, data.surface_color);
                    return LRESULT(get_surface_brush(data).0 as isize);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_COMMAND => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &mut *app_data;
                let hwnd_ctl = HWND(lparam.0 as isize);
                let notify = ((wparam.0 >> 16) & 0xFFFF) as u16;

                if notify == BN_CLICKED as u16 {
                    handle_button_click(hwnd_ctl, data);
                } else if notify == CBN_SELCHANGE as u16 && hwnd_ctl == data.hwnd_combo_source {
                    let sel = SendMessageW(data.hwnd_combo_source, 0x014E, WPARAM(0), LPARAM(0)).0 as i32;
                    data.state.use_winget = sel == 1;
                    let cb = data.callbacks.on_save_settings.clone();
                    cb(sel == 1);
                }
            }
            LRESULT(0)
        }
        WM_UI_EVENT => {
            let app_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !app_data.is_null() {
                let data = &mut *app_data;
                for event in events::drain_events() {
                    match event {
                        UiEvent::SetInstallStatus(s) => data.state.install_status = s,
                        UiEvent::SetTaskInfo(s) => data.state.task_info = s,
                        UiEvent::SetControlsEnabled(v) => data.state.controls_enabled = v,
                        UiEvent::SetIsBusy(v) => data.state.is_busy = v,
                        UiEvent::SetInterceptorEnabled(v) => data.state.interceptor_enabled = v,
                        UiEvent::SetInstallScope(s) => data.state.install_scope = s,
                        UiEvent::SetHeliumInstalled(v) => data.state.helium_installed = v,
                    }
                }
                sync_controls(data);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn create_controls(hwnd: HWND, data: &mut AppData, hinstance: HINSTANCE) {
    let dpi = match GetDpiForWindow(hwnd) {
        0 => 96,
        value => value as i32,
    };
    let scale = |value: i32| scale_for_dpi(value, dpi);
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let _ = SystemParametersInfoW(
        SPI_GETNONCLIENTMETRICS,
        metrics.cbSize,
        Some(&mut metrics as *mut _ as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );

    let message_font = metrics.lfMessageFont;
    data.ui_font = CreateFontIndirectW(&message_font);

    let mut title_font: LOGFONTW = message_font;
    title_font.lfHeight = message_font.lfHeight.saturating_mul(3) / 2;
    title_font.lfWeight = 700;
    data.title_font = CreateFontIndirectW(&title_font);

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn apply_font(hwnd: HWND, font: HFONT) {
        if hwnd.0 != 0 && font.0 != 0 {
            SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }

    let ex = WINDOW_EX_STYLE(0);
    let vis = WS_CHILD | WS_VISIBLE;
    let margin = scale(12);
    let content_width = scale(CLIENT_WIDTH - 24);
    let inner_left = margin + scale(12);
    let combo_width = scale(180);
    let right_button_x = margin + content_width - combo_width - scale(8);
    let small_button_gap = scale(6);
    let help_button_w = scale(22);
    let action_button_w = scale(73);
    let help_button_x = margin + content_width - scale(8) - help_button_w;
    let restore_button_x = help_button_x - small_button_gap - action_button_w;
    let enable_button_x = restore_button_x - small_button_gap - action_button_w;
    let title_y = scale(10);
    let title_h = scale(30);
    let subtitle_y = title_y + title_h + scale(2);
    let subtitle_h = scale(22);
    let section_title_h = scale(20);
    let frame_gap = scale(4);
    let helium_title_y = subtitle_y + subtitle_h + scale(12);
    let helium_y = helium_title_y + section_title_h + frame_gap;
    let helium_h = scale(76);
    let helium_row_y = helium_y + scale(12);
    let helium_row2_y = helium_row_y + scale(24);
    let config_title_y = helium_y + helium_h + scale(12);
    let config_y = config_title_y + section_title_h + frame_gap;
    let config_h = scale(122);
    let source_row_y = config_y + scale(14);
    let route_row_y = config_y + scale(48);
    let uninstall_row_y = config_y + scale(82);

    data.hwnd_title = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("HUpdater").as_ptr()),
        vis, margin, title_y, scale(300), title_h,
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_title, data.title_font);

    data.hwnd_subtitle = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Autoupdates Helium while it is still in alpha. Settings apply automatically.").as_ptr()),
        vis, margin, subtitle_y, scale(452), subtitle_h,
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_subtitle, data.ui_font);

    data.hwnd_helium_title = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Helium").as_ptr()),
        vis, margin, helium_title_y, scale(56), section_title_h,
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_helium_title, data.ui_font);

    data.hwnd_group_helium = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("").as_ptr()),
        vis, margin, helium_y, content_width, helium_h,
        hwnd, None, hinstance, None,
    );

    data.hwnd_status_label = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Status: Checking...").as_ptr()),
        vis, inner_left, helium_row_y, scale(292), scale(20),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_status_label, data.ui_font);

    data.hwnd_version_label = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("").as_ptr()),
        vis, inner_left, helium_row2_y, scale(292), scale(20),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_version_label, data.ui_font);

    data.hwnd_check_install = CreateWindowExW(
        ex, PCWSTR(w("Button").as_ptr()), PCWSTR(w("Check / Install").as_ptr()),
        vis | WS_TABSTOP, right_button_x, helium_row_y + scale(6), combo_width, scale(30),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_check_install, data.ui_font);

    data.hwnd_config_title = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Configuration").as_ptr()),
        vis, margin, config_title_y, scale(96), section_title_h,
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_config_title, data.ui_font);

    data.hwnd_group_config = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("").as_ptr()),
        vis, margin, config_y, content_width, config_h,
        hwnd, None, hinstance, None,
    );

    data.hwnd_source_label = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Preferred update source").as_ptr()),
        vis, inner_left, source_row_y, scale(210), scale(24),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_source_label, data.ui_font);

    data.hwnd_combo_source = CreateWindowExW(
        ex, PCWSTR(w("ComboBox").as_ptr()), PCWSTR(std::ptr::null()),
        vis | WS_TABSTOP | CBS_DROPDOWNLIST, margin + content_width - combo_width - scale(8), source_row_y - scale(2), combo_width, scale(220),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_combo_source, data.ui_font);
    let item0 = w("GitHub Installer");
    let item1 = w("Winget");
    SendMessageW(data.hwnd_combo_source, CB_ADDSTRING, WPARAM(0), LPARAM(item0.as_ptr() as isize));
    SendMessageW(data.hwnd_combo_source, CB_ADDSTRING, WPARAM(0), LPARAM(item1.as_ptr() as isize));
    SendMessageW(data.hwnd_combo_source, CB_SETCURSEL, WPARAM(0), LPARAM(0));

    data.hwnd_interceptor_label = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Route shortcuts through updater").as_ptr()),
        vis, inner_left, route_row_y, scale(248), scale(24),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_interceptor_label, data.ui_font);

    data.hwnd_enable = CreateWindowExW(
        ex, PCWSTR(w("Button").as_ptr()), PCWSTR(w("Enable").as_ptr()),
        vis | WS_TABSTOP, enable_button_x, route_row_y - scale(2), action_button_w, scale(28),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_enable, data.ui_font);

    data.hwnd_restore = CreateWindowExW(
        ex, PCWSTR(w("Button").as_ptr()), PCWSTR(w("Restore").as_ptr()),
        vis | WS_TABSTOP, restore_button_x, route_row_y - scale(2), action_button_w, scale(28),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_restore, data.ui_font);

    data.hwnd_help = CreateWindowExW(
        ex, PCWSTR(w("Button").as_ptr()), PCWSTR(w("(?)").as_ptr()),
        vis | WS_TABSTOP, help_button_x, route_row_y - scale(2), help_button_w, scale(28),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_help, data.ui_font);

    data.hwnd_uninstall_label = CreateWindowExW(
        ex, PCWSTR(w("Static").as_ptr()), PCWSTR(w("Remove updater from system").as_ptr()),
        vis, inner_left, uninstall_row_y, scale(248), scale(24),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_uninstall_label, data.ui_font);

    data.hwnd_uninstall = CreateWindowExW(
        ex, PCWSTR(w("Button").as_ptr()), PCWSTR(w("Uninstall").as_ptr()),
        vis | WS_TABSTOP, right_button_x, uninstall_row_y - scale(2), combo_width, scale(28),
        hwnd, None, hinstance, None,
    );
    apply_font(data.hwnd_uninstall, data.ui_font);

    sync_controls(data);
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn sync_controls(data: &mut AppData) {
    let s = &data.state;

    let (status_line, version_line) = match s.install_status.split_once(" | ") {
        Some((status, version)) => (format!("Status: {}", status), version.to_string()),
        None => (format!("Status: {}", s.install_status), String::new()),
    };

    let wide: Vec<u16> = status_line.encode_utf16().chain(std::iter::once(0)).collect();
    SetWindowTextW(data.hwnd_status_label, PCWSTR(wide.as_ptr()));

    let wide: Vec<u16> = version_line.encode_utf16().chain(std::iter::once(0)).collect();
    SetWindowTextW(data.hwnd_version_label, PCWSTR(wide.as_ptr()));

    let btn_text = if s.is_busy { "Processing..." } else { "Check / Install" };
    let wide: Vec<u16> = btn_text.encode_utf16().chain(std::iter::once(0)).collect();
    SetWindowTextW(data.hwnd_check_install, PCWSTR(wide.as_ptr()));

    EnableWindow(data.hwnd_check_install, if s.is_busy { 0 } else { 1 });
    let config_enabled = s.controls_enabled && s.helium_installed;
    EnableWindow(data.hwnd_combo_source, if config_enabled { 1 } else { 0 });
    EnableWindow(data.hwnd_enable, if config_enabled && !s.interceptor_enabled { 1 } else { 0 });
    EnableWindow(data.hwnd_restore, if config_enabled && s.interceptor_enabled { 1 } else { 0 });
    EnableWindow(data.hwnd_uninstall, if config_enabled { 1 } else { 0 });

    let combo_index = if s.use_winget { 1 } else { 0 };
    SendMessageW(data.hwnd_combo_source, CB_SETCURSEL, WPARAM(combo_index), LPARAM(0));

}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn handle_button_click(hwnd: HWND, data: &mut AppData) {
    if hwnd == data.hwnd_help {
        let title = to_wide_null("Interception Guide");
        let body = to_wide_null("Turn this on so Helium updates automatically whenever you open it from a shortcut.\n\nMake sure your Helium shortcuts already exist before you click Enable. If you add new ones later — like pinning to the taskbar — come back here and click Enable again.");
        let _ = windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            HWND(0),
            windows::core::PCWSTR(body.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            windows::Win32::UI::WindowsAndMessaging::MB_OK | windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
        );
    } else if hwnd == data.hwnd_check_install {
        data.state.is_busy = true;
        data.state.task_info = "Updating...".to_string();
        sync_controls(data);
        let cb = data.callbacks.on_trigger_update.clone();
        cb();
    } else if hwnd == data.hwnd_enable {
        data.state.is_busy = true;
        data.state.task_info = "Enabling interceptor...".to_string();
        sync_controls(data);
        let cb = data.callbacks.on_enable_interceptor.clone();
        cb();
    } else if hwnd == data.hwnd_restore {
        data.state.is_busy = true;
        data.state.task_info = "Restoring interceptor...".to_string();
        sync_controls(data);
        let cb = data.callbacks.on_restore_interceptor.clone();
        cb();
    } else if hwnd == data.hwnd_uninstall {
        data.state.task_info = "Uninstalling...".to_string();
        sync_controls(data);
        let cb = data.callbacks.on_uninstall_updater.clone();
        cb();
    }
}
