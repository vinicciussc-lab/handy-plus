//! Focused text box detection (UI Automation) for the floating button.

/// The text box that has keyboard focus, as reported by UI Automation.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldInfo {
    /// Top-level window that owns the field (used to refocus it later).
    pub hwnd: isize,
    pub pid: u32,
    /// Executable file name only, e.g. `Discord.exe`.
    pub process_name: String,
    /// left, top, right, bottom in physical pixels.
    pub rect: (i32, i32, i32, i32),
    pub editable: bool,
    pub password: bool,
    /// Only probed for editable, non-password fields; `false` otherwise.
    pub has_text: bool,
}

/// Reads the focused element. Any error, or nothing visible focused, gives `None`.
///
/// Call it from a background thread (never the app's UI thread): COM is
/// initialized there as MTA and the UI Automation client is created once per
/// thread. Calls into the target app are capped by short UIA timeouts.
pub fn focused_field() -> Option<FieldInfo> {
    #[cfg(windows)]
    {
        win::focused_field()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Whether the floating button should be visible for `field` right now.
pub fn should_show_button(
    field: Option<&FieldInfo>,
    enabled: bool,
    session_active: bool,
    fullscreen: bool,
    excluded: &[String],
    own_pid: u32,
) -> bool {
    if !enabled || session_active || fullscreen {
        return false;
    }
    let Some(field) = field else {
        return false;
    };
    field.editable
        && !field.password
        && field.has_text
        && field.pid != own_pid
        && !excluded
            .iter()
            .any(|app| app.trim().eq_ignore_ascii_case(&field.process_name))
}

/// Top-left corner of the button: inside the bottom-right corner of the box,
/// or just to the right of the box (bottom-aligned) when the box is too small.
pub fn button_position(rect: (i32, i32, i32, i32), size_px: i32, inset_px: i32) -> (i32, i32) {
    let (left, top, right, bottom) = rect;
    let needed = size_px + 2 * inset_px;
    if right - left < needed || bottom - top < needed {
        (right + inset_px, bottom - size_px)
    } else {
        (right - size_px - inset_px, bottom - size_px - inset_px)
    }
}

#[cfg(windows)]
mod win {
    use super::FieldInfo;
    use std::cell::RefCell;
    use windows::core::{Interface, BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::System::Variant::{VariantClear, VT_BOOL};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
        IUIAutomationTextPattern, IUIAutomationTextRange, IUIAutomationValuePattern,
        UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId,
        UIA_TextPatternId, UIA_ValuePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GA_ROOT};

    /// Caps for cross-process UI Automation calls, so a hung target app
    /// cannot stall the polling thread (the defaults are 2 s / 20 s).
    const CONNECTION_TIMEOUT_MS: u32 = 100;
    const TRANSACTION_TIMEOUT_MS: u32 = 50;

    thread_local! {
        static AUTOMATION: RefCell<Option<IUIAutomation>> = const { RefCell::new(None) };
    }

    pub fn focused_field() -> Option<FieldInfo> {
        let automation = AUTOMATION.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = create_automation();
            }
            slot.clone()
        })?;
        // SAFETY: COM calls on a live interface; every result is checked.
        unsafe { read_focused(&automation) }
    }

    fn create_automation() -> Option<IUIAutomation> {
        // SAFETY: COM init for this thread. S_FALSE / RPC_E_CHANGED_MODE mean
        // COM is already usable on it, so the result is ignored on purpose.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(a2) =
                CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
            {
                let _ = a2.SetConnectionTimeout(CONNECTION_TIMEOUT_MS);
                let _ = a2.SetTransactionTimeout(TRANSACTION_TIMEOUT_MS);
                return a2.cast::<IUIAutomation>().ok();
            }
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        }
    }

    unsafe fn read_focused(automation: &IUIAutomation) -> Option<FieldInfo> {
        unsafe {
            let el = automation.GetFocusedElement().ok()?;
            let control = el.CurrentControlType().ok()?;
            let is_edit = control == UIA_EditControlTypeId;
            let is_doc = control == UIA_DocumentControlTypeId;

            let r = el.CurrentBoundingRectangle().ok()?;
            if r.right <= r.left || r.bottom <= r.top {
                return None;
            }
            let pid = u32::try_from(el.CurrentProcessId().ok()?).ok()?;
            let enabled = el.CurrentIsEnabled().is_ok_and(BOOL::as_bool);
            let password = el.CurrentIsPassword().is_ok_and(BOOL::as_bool);

            let mut editable = false;
            let mut has_text = false;
            if (is_edit || is_doc) && enabled {
                let value: Option<IUIAutomationValuePattern> =
                    el.GetCurrentPatternAs(UIA_ValuePatternId).ok();
                let range = document_range(&el);
                let mut read_only = value
                    .as_ref()
                    .is_some_and(|v| v.CurrentIsReadOnly().is_ok_and(BOOL::as_bool));
                // Browsers focus a read-only Document for the page itself; only
                // treat a Document as editable when its text says so explicitly.
                if is_doc && !read_only {
                    read_only = !range.as_ref().is_some_and(|r| explicitly_writable(r));
                }
                editable = !read_only;
                // Never read the contents of a password field.
                if editable && !password {
                    let from_value = || {
                        value
                            .as_ref()
                            .and_then(|v| v.CurrentValue().ok())
                            .map(|b| !b.is_empty())
                    };
                    let from_range = || {
                        range
                            .as_ref()
                            .and_then(|r| r.GetText(1).ok())
                            .map(|b| !b.is_empty())
                    };
                    has_text = if is_doc {
                        from_range().or_else(from_value)
                    } else {
                        from_value().or_else(from_range)
                    }
                    .unwrap_or(false);
                }
            }

            let native = el.CurrentNativeWindowHandle().unwrap_or_default();
            let owner = if native.is_invalid() {
                GetForegroundWindow()
            } else {
                native
            };
            let root = GetAncestor(owner, GA_ROOT);
            let hwnd: HWND = if root.is_invalid() { owner } else { root };

            Some(FieldInfo {
                hwnd: hwnd.0 as isize,
                pid,
                process_name: process_name(pid).unwrap_or_default(),
                rect: (r.left, r.top, r.right, r.bottom),
                editable,
                password,
                has_text,
            })
        }
    }

    unsafe fn document_range(el: &IUIAutomationElement) -> Option<IUIAutomationTextRange> {
        unsafe {
            el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok()?
                .DocumentRange()
                .ok()
        }
    }

    /// True only when the range reports `IsReadOnly == false` as a plain bool
    /// (not "mixed" and not "not supported").
    unsafe fn explicitly_writable(range: &IUIAutomationTextRange) -> bool {
        unsafe {
            let Ok(mut v) = range.GetAttributeValue(UIA_IsReadOnlyAttributeId) else {
                return false;
            };
            let inner = &v.Anonymous.Anonymous;
            let writable = inner.vt == VT_BOOL && !inner.Anonymous.boolVal.as_bool();
            // The raw VARIANT has no Drop; clear it so a mixed/unsupported
            // IUnknown sentinel is released.
            let _ = VariantClear(&mut v);
            writable
        }
    }

    fn process_name(pid: u32) -> Option<String> {
        // SAFETY: the handle is closed before returning; the buffer outlives the call.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_string)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correction::settings::CorrectionSettings;

    fn f() -> FieldInfo {
        FieldInfo {
            hwnd: 1,
            pid: 10,
            process_name: "notepad.exe".into(),
            rect: (100, 100, 500, 200),
            editable: true,
            password: false,
            has_text: true,
        }
    }

    #[test]
    fn shows_for_editable_field_with_text() {
        assert!(should_show_button(Some(&f()), true, false, false, &[], 99));
    }

    #[test]
    fn hides_for_every_blocker() {
        assert!(!should_show_button(None, true, false, false, &[], 99));
        assert!(!should_show_button(
            Some(&f()),
            false,
            false,
            false,
            &[],
            99
        ));
        assert!(!should_show_button(Some(&f()), true, true, false, &[], 99));
        assert!(!should_show_button(Some(&f()), true, false, true, &[], 99));
        assert!(!should_show_button(
            Some(&FieldInfo {
                password: true,
                ..f()
            }),
            true,
            false,
            false,
            &[],
            99
        ));
        assert!(!should_show_button(
            Some(&FieldInfo {
                has_text: false,
                ..f()
            }),
            true,
            false,
            false,
            &[],
            99
        ));
        assert!(!should_show_button(
            Some(&FieldInfo {
                editable: false,
                ..f()
            }),
            true,
            false,
            false,
            &[],
            99
        ));
        assert!(!should_show_button(
            Some(&f()),
            true,
            false,
            false,
            &["NOTEPAD.EXE".into()],
            99
        ));
        assert!(!should_show_button(Some(&f()), true, false, false, &[], 10));
    }

    #[test]
    fn button_sits_inside_bottom_right_or_beside_small_fields() {
        assert_eq!(button_position((100, 100, 500, 200), 28, 6), (466, 166));
        assert_eq!(button_position((100, 100, 130, 120), 28, 6), (136, 92));
    }

    #[test]
    fn floating_button_defaults_on_with_no_exclusions() {
        let s = CorrectionSettings::default();
        assert!(s.floating_button);
        assert!(s.button_excluded_apps.is_empty());
    }

    /// Manual: the user focuses a text box (e.g. Notepad) by hand; this only
    /// prints `focused_field()` every 500 ms for 10 s and never automates the desktop.
    #[test]
    #[ignore]
    fn print_focused_field() {
        for _ in 0..20 {
            let start = std::time::Instant::now();
            let field = focused_field();
            println!("{:>4} ms  {:?}", start.elapsed().as_millis(), field);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
