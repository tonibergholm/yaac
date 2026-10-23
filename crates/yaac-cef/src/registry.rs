//! Browser-id to view attribution. Plain data, readable from any thread: the scheme handler runs
//! on the IO thread and asks here who is calling.

use std::collections::HashMap;
use std::sync::Mutex;
use yaac_engine::ViewKind;
use yaac_model::ViewId;

#[derive(Default)]
pub(crate) struct Registry {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    views: HashMap<ViewId, ViewKind>,
    browsers: HashMap<i32, (ViewId, ViewKind)>,
}

impl Registry {
    pub(crate) fn add_view(&self, view: ViewId, kind: ViewKind) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.views.insert(view, kind);
        }
    }

    pub(crate) fn remove_view(&self, view: ViewId) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.views.remove(&view);
        }
    }

    pub(crate) fn bind_browser(&self, browser: i32, view: ViewId, kind: ViewKind) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.browsers.insert(browser, (view, kind));
        }
    }

    pub(crate) fn unbind_browser(&self, browser: i32) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.browsers.remove(&browser);
        }
    }

    /// The view and kind behind a browser id. `None` means the browser cannot be attributed.
    pub(crate) fn browser(&self, browser: i32) -> Option<(ViewId, ViewKind)> {
        self.inner.lock().ok()?.browsers.get(&browser).copied()
    }

    /// The kind of a view the core created and has not closed.
    pub(crate) fn view_kind(&self, view: ViewId) -> Option<ViewKind> {
        self.inner.lock().ok()?.views.get(&view).copied()
    }
}
