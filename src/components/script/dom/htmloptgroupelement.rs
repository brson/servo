/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use dom::bindings::utils::{DOMString, ErrorResult, null_string};
use dom::htmlelement::HTMLElement;

pub struct HTMLOptGroupElement {
    parent: HTMLElement
}

impl HTMLOptGroupElement {
    pub fn Disabled(&self) -> bool {
        false
    }

    pub fn SetDisabled(&mut self, _disabled: bool, _rv: &mut ErrorResult) {
    }

    pub fn Label(&self) -> DOMString {
        null_string
    }

    pub fn SetLabel(&mut self, _label: &DOMString, _rv: &mut ErrorResult) {
    }
}
