//! AppKit backdrop behind the existing Surfman view; no renderer replacement.
#[cfg(target_os = "macos")]
mod platform {
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    pub struct NativeTheme {
        effect: Retained<NSVisualEffectView>,
    }
    impl NativeTheme {
        pub fn new(window: &winit::window::Window) -> Result<Self, String> {
            let mtm = MainThreadMarker::new().ok_or("AppKit themes require the main thread")?;
            let RawWindowHandle::AppKit(handle) =
                window.window_handle().map_err(|e| e.to_string())?.as_raw()
            else {
                return Err("Expected an AppKit window".into());
            };
            // Winit owns this NSView for the lifetime of the window. The effect is a
            // sibling below it, so neither mouse handling nor Surfman's layers change.
            let root = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            let parent = unsafe { root.superview() }.ok_or("The native view has no parent")?;
            let effect =
                NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), root.frame());
            effect.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            effect.setMaterial(NSVisualEffectMaterial::Sidebar);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect.setState(NSVisualEffectState::FollowsWindowActiveState);
            effect.setHidden(true);
            parent.addSubview_positioned_relativeTo(
                &effect,
                NSWindowOrderingMode::Below,
                Some(root),
            );
            Ok(Self { effect })
        }
        pub fn available(&self) -> bool {
            true
        }
        pub fn apply(&self, theme: crate::theme::Theme, dark: bool) {
            self.effect.setHidden(!theme.glass());
            let name = unsafe {
                if dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            let appearance = if theme == crate::theme::Theme::Glass {
                None
            } else {
                NSAppearance::appearanceNamed(name)
            };
            self.effect.setAppearance(appearance.as_deref());
            if let Some(window) = self.effect.window() {
                window.setAppearance(appearance.as_deref());
                // Keep the window nonopaque in every theme: Surfman records this
                // at creation. Opaque themes fill the framebuffer themselves.
                window.setOpaque(false);
                window.setBackgroundColor(Some(&NSColor::clearColor()));
            }
        }
    }
    impl Drop for NativeTheme {
        fn drop(&mut self) {
            self.effect.removeFromSuperview();
        }
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    pub struct NativeTheme;
    impl NativeTheme {
        pub fn new(_: &winit::window::Window) -> Result<Self, String> {
            Err("Native vibrancy is available on macOS".into())
        }
        pub fn available(&self) -> bool {
            false
        }
        pub fn apply(&self, _: crate::theme::Theme, _: bool) {}
    }
}
#[cfg(target_os = "windows")]
mod platform {
    use std::{cell::Cell, mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute},
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            Controls::MARGINS,
            WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
        },
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    fn transparency_enabled() -> bool {
        let mut contrast = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        unsafe {
            if SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&mut contrast as *mut HIGHCONTRASTW).cast(),
                0,
            ) == 0
                || contrast.dwFlags & HCF_HIGHCONTRASTON != 0
            {
                return false;
            }
            let key: Vec<_> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
                .encode_utf16()
                .collect();
            let name: Vec<_> = "EnableTransparency\0".encode_utf16().collect();
            let mut enabled = 1_u32;
            let mut size = size_of::<u32>() as u32;
            let result = RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&mut enabled as *mut u32).cast(),
                &mut size,
            );
            // Missing preference uses the OS default. Other read failures use opaque chrome.
            (result == 0 && enabled != 0) || result == 2
        }
    }
    pub struct NativeTheme {
        hwnd: HWND,
        available: Cell<bool>,
        glass: Cell<bool>,
    }
    impl NativeTheme {
        pub fn new(window: &winit::window::Window) -> Result<Self, String> {
            let RawWindowHandle::Win32(handle) =
                window.window_handle().map_err(|e| e.to_string())?.as_raw()
            else {
                return Err("Expected a Win32 window".into());
            };
            let hwnd = handle.hwnd.get() as HWND;
            let none = 1_u32; // DWMSBT_NONE; attribute 38 requires Windows 11 22H2.
            if unsafe { DwmSetWindowAttribute(hwnd, 38, (&none as *const u32).cast(), 4) } < 0 {
                return Err("System backdrop unavailable; using opaque themes".into());
            }
            Ok(Self {
                hwnd,
                available: Cell::new(transparency_enabled()),
                glass: Cell::new(false),
            })
        }
        pub fn available(&self) -> bool {
            self.available.get()
        }
        pub fn apply(&self, theme: crate::theme::Theme, dark: bool) {
            let enabled = transparency_enabled();
            let glass = theme.glass() && enabled;
            let material = if glass { 2_u32 } else { 1 }; // Main-window Mica / none.
            let margins = MARGINS {
                cxLeftWidth: if glass { -1 } else { 0 },
                cxRightWidth: if glass { -1 } else { 0 },
                cyTopHeight: if glass { -1 } else { 0 },
                cyBottomHeight: if glass { -1 } else { 0 },
            };
            let dark = i32::from(dark);
            let success = unsafe {
                DwmSetWindowAttribute(self.hwnd, 20, (&dark as *const i32).cast(), 4);
                DwmExtendFrameIntoClientArea(self.hwnd, &margins) >= 0
                    && DwmSetWindowAttribute(self.hwnd, 38, (&material as *const u32).cast(), 4)
                        >= 0
            };
            self.available.set(enabled && success);
            self.glass.set(glass && success);
        }
    }
}
pub use platform::NativeTheme;
