use rustdoc_core::{Navigator, RustdocProject};
use std::cell::{Ref, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use crate::format_context::FormatContext;

/// wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    navigator: Navigator,
    format_context: RefCell<FormatContext>,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        &self.navigator
    }
}

impl Request {
    /// Create a new request for a project
    pub(crate) fn new(project: RustdocProject, format_context: FormatContext) -> Self {
        Self {
            navigator: Navigator::new(Rc::new(project)),
            format_context: RefCell::new(format_context),
        }
    }

    pub(crate) fn mutate_format_context(&self, f: impl FnOnce(&mut FormatContext)) {
        let mut b = self.format_context.borrow_mut();
        f(&mut b);
    }

    pub(crate) fn format_context(&self) -> Ref<'_, FormatContext> {
        self.format_context.borrow()
    }
}
