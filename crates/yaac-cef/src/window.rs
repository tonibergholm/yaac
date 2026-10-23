//! Window and browser-view delegates. The window closes through `can_close`, which asks each
//! browser to close first.

use crate::ui;
use cef::*;
use yaac_model::WindowId;

wrap_window_delegate! {
    pub(crate) struct WindowDelegateImpl {
        window: WindowId,
        root: Panel,
        width: i32,
        height: i32,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: self.width, height: self.height }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else { return };
            let mut root = View::from(&self.root);
            window.add_child_view(Some(&mut root));
            crate::os::fit_window(window, self.width, self.height);
            window.show();
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            ui::window_destroyed(self.window);
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            i32::from(ui::can_close_window(self.window))
        }

        fn linux_window_properties(&self, _window: Option<&mut Window>, properties: Option<&mut LinuxWindowProperties>) -> i32 {
            properties.map_or(0, crate::os::fill_window_properties)
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

wrap_browser_view_delegate! {
    pub(crate) struct ViewDelegateImpl {
        width: i32,
        height: i32,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: self.width, height: self.height }
        }
    }

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}
