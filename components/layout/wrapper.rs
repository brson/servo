/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! A safe wrapper for DOM nodes that prevents layout from mutating the DOM, from letting DOM nodes
//! escape, and from generally doing anything that it isn't supposed to. This is accomplished via
//! a simple whitelist of allowed operations, along with some lifetime magic to prevent nodes from
//! escaping.
//!
//! As a security wrapper is only as good as its whitelist, be careful when adding operations to
//! this list. The cardinal rules are:
//!
//! 1. Layout is not allowed to mutate the DOM.
//!
//! 2. Layout is not allowed to see anything with `LayoutJS` in the name, because it could hang
//!    onto these objects and cause use-after-free.
//!
//! When implementing wrapper functions, be careful that you do not touch the borrow flags, or you
//! will race and cause spurious task failure. (Note that I do not believe these races are
//! exploitable, but they'll result in brokenness nonetheless.)
//!
//! Rules of the road for this file:
//!
//! * You must not use `.get()`; instead, use `.unsafe_get()`.
//!
//! * Do not call any methods on DOM nodes without checking to see whether they use borrow flags.
//!
//!   o Instead of `get_attr()`, use `.get_attr_val_for_layout()`.
//!
//!   o Instead of `html_element_in_html_document()`, use
//!     `html_element_in_html_document_for_layout()`.

#![allow(unsafe_code)]

use context::SharedLayoutContext;
use css::node_style::StyledNode;
use incremental::RestyleDamage;
use data::{LayoutDataAccess, LayoutDataFlags, LayoutDataWrapper, PrivateLayoutData};
use opaque_node::OpaqueNodeMethods;

use gfx::display_list::OpaqueNode;
use script::dom::bindings::codegen::InheritTypes::{CharacterDataCast};
use script::dom::bindings::codegen::InheritTypes::{TextCast};
use script::dom::bindings::js::LayoutJS;
use script::dom::characterdata::{LayoutCharacterDataHelpers};
use script::dom::node::{LayoutNodeHelpers};
use script::dom::text::Text;
use layout_traits::layout_interface::{LayoutChan, SharedLayoutData};
use util::str::{is_whitespace};
use std::cell::{Ref, RefMut};
use std::mem;
use style::computed_values::content::ContentItem;
use style::computed_values::{display, white_space};
use style::node::{TNode};

use script::layout_dom::{LayoutNode, ThreadSafeLayoutNode, PostorderNodeMutTraversal};
use script::layout_dom::{PseudoElementType, get_content, TLayoutNode};
use script::layout_dom::TLayoutNode2 as ScriptTLayoutNode2;


// Extracted from layout::wrapper::TLayoutNode for ThreadSafeLayoutNode
pub trait TLayoutNode2<'ln> {
    /// Returns the first child of this node.
    fn first_child(&self) -> Option<Self>;

    /// If this is a text node or generated content, copies out its content. If this is not a text
    /// node, fails.
    ///
    /// FIXME(pcwalton): This might have too much copying and/or allocation. Profile this.
    fn text_content(&self) -> Vec<ContentItem>;
}

pub trait LayoutNodeExt<'ln> {
    fn initialize_layout_data(self, chan: LayoutChan);
    fn layout_parent_node(self, shared: &SharedLayoutContext) -> Option<LayoutNode<'ln>>;
    fn debug_id(self) -> usize;
    fn flow_debug_id(self) -> usize;
}

// Extracted from `impl layout::wrapper::LayoutNode`
impl<'ln> LayoutNodeExt<'ln> for LayoutNode<'ln> {
    /// Resets layout data and styles for the node.
    ///
    /// FIXME(pcwalton): Do this as part of fragment building instead of in a traversal.
    fn initialize_layout_data(self, chan: LayoutChan) {
        let mut layout_data_ref = self.mutate_layout_data();
        match *layout_data_ref {
            None => {
                *layout_data_ref = Some(LayoutDataWrapper {
                    chan: Some(chan),
                    shared_data: SharedLayoutData { style: None },
                    data: box PrivateLayoutData::new(),
                });
            }
            Some(_) => {}
        }
    }

    /// While doing a reflow, the node at the root has no parent, as far as we're
    /// concerned. This method returns `None` at the reflow root.
    fn layout_parent_node(self, shared: &SharedLayoutContext) -> Option<LayoutNode<'ln>> {
        match shared.reflow_root {
            None => panic!("layout_parent_node(): This layout has no access to the DOM!"),
            Some(reflow_root) => {
                let opaque_node: OpaqueNode = OpaqueNodeMethods::from_layout_node(&self);
                if opaque_node == reflow_root {
                    None
                } else {
                    self.parent_node()
                }
            }
        }
    }

    fn debug_id(self) -> usize {
        let opaque: OpaqueNode = OpaqueNodeMethods::from_layout_node(&self);
        opaque.to_untrusted_node_address().0 as usize
    }

    fn flow_debug_id(self) -> usize {
        let layout_data_ref = self.borrow_layout_data();
        match *layout_data_ref {
            None => 0,
            Some(ref layout_data) => layout_data.data.flow_construction_result.debug_id()
        }
    }

}

pub trait ThreadSafeLayoutNodeExt<'ln> {
    fn debug_id(self) -> usize;
    fn flow_debug_id(self) -> usize;
    fn borrow_layout_data_unchecked<'a>(&'a self) -> *const Option<LayoutDataWrapper>;
    fn borrow_layout_data<'a>(&'a self) -> Ref<'a,Option<LayoutDataWrapper>>;
    fn mutate_layout_data<'a>(&'a self) -> RefMut<'a,Option<LayoutDataWrapper>>;
    fn restyle_damage(self) -> RestyleDamage;
    fn set_restyle_damage(self, damage: RestyleDamage);
    fn flags(self) -> LayoutDataFlags;
    fn insert_flags(self, new_flags: LayoutDataFlags);
    fn remove_flags(self, flags: LayoutDataFlags);
    fn get_normal_display(&self) -> display::T;
    fn get_before_display(&self) -> display::T;
    fn get_after_display(&self) -> display::T;
    fn has_before_pseudo(&self) -> bool;
    fn has_after_pseudo(&self) -> bool;
    fn children(&self) -> ThreadSafeLayoutNodeChildrenIterator<'ln>;
    fn traverse_postorder_mut<T:PostorderNodeMutTraversal>(&mut self, traversal: &mut T) -> bool;
    fn is_ignorable_whitespace(&self) -> bool;
}

// Extracted from `impl layout::wrapper::ThreadSafeLayoutNode`
impl<'ln> ThreadSafeLayoutNodeExt<'ln> for ThreadSafeLayoutNode<'ln> {
    fn debug_id(self) -> usize {
        self.node.debug_id()
    }

    fn flow_debug_id(self) -> usize {
        self.node.flow_debug_id()
    }

    /// Borrows the layout data without checking. Fails on a conflicting borrow.
    #[inline(always)]
    fn borrow_layout_data_unchecked<'a>(&'a self) -> *const Option<LayoutDataWrapper> {
        unsafe {
            mem::transmute(self.get().layout_data_unchecked())
        }
    }

    /// Borrows the layout data immutably. Fails on a conflicting borrow.
    ///
    /// TODO(pcwalton): Make this private. It will let us avoid borrow flag checks in some cases.
    #[inline(always)]
    fn borrow_layout_data<'a>(&'a self) -> Ref<'a,Option<LayoutDataWrapper>> {
        unsafe {
            mem::transmute(self.get().layout_data())
        }
    }

    /// Borrows the layout data mutably. Fails on a conflicting borrow.
    ///
    /// TODO(pcwalton): Make this private. It will let us avoid borrow flag checks in some cases.
    #[inline(always)]
    fn mutate_layout_data<'a>(&'a self) -> RefMut<'a,Option<LayoutDataWrapper>> {
        unsafe {
            mem::transmute(self.get().layout_data_mut())
        }
    }

    /// Get the description of how to account for recent style changes.
    /// This is a simple bitfield and fine to copy by value.
    fn restyle_damage(self) -> RestyleDamage {
        let layout_data_ref = self.borrow_layout_data();
        layout_data_ref.as_ref().unwrap().data.restyle_damage
    }

    /// Set the restyle damage field.
    fn set_restyle_damage(self, damage: RestyleDamage) {
        let mut layout_data_ref = self.mutate_layout_data();
        match &mut *layout_data_ref {
            &mut Some(ref mut layout_data) => layout_data.data.restyle_damage = damage,
            _ => panic!("no layout data for this node"),
        }
    }

    /// Returns the layout data flags for this node.
    fn flags(self) -> LayoutDataFlags {
        unsafe {
            match *self.borrow_layout_data_unchecked() {
                None => panic!(),
                Some(ref layout_data) => layout_data.data.flags,
            }
        }
    }

    /// Adds the given flags to this node.
    fn insert_flags(self, new_flags: LayoutDataFlags) {
        let mut layout_data_ref = self.mutate_layout_data();
        match &mut *layout_data_ref {
            &mut Some(ref mut layout_data) => layout_data.data.flags.insert(new_flags),
            _ => panic!("no layout data for this node"),
        }
    }

    /// Removes the given flags from this node.
    fn remove_flags(self, flags: LayoutDataFlags) {
        let mut layout_data_ref = self.mutate_layout_data();
        match &mut *layout_data_ref {
            &mut Some(ref mut layout_data) => layout_data.data.flags.remove(flags),
            _ => panic!("no layout data for this node"),
        }
    }

    #[inline]
    fn get_normal_display(&self) -> display::T {
        let mut layout_data_ref = self.mutate_layout_data();
        let node_layout_data_wrapper = layout_data_ref.as_mut().unwrap();
        let style = node_layout_data_wrapper.shared_data.style.as_ref().unwrap();
        style.get_box().display
    }

    #[inline]
    fn get_before_display(&self) -> display::T {
        let mut layout_data_ref = self.mutate_layout_data();
        let node_layout_data_wrapper = layout_data_ref.as_mut().unwrap();
        let style = node_layout_data_wrapper.data.before_style.as_ref().unwrap();
        style.get_box().display
    }

    #[inline]
    fn get_after_display(&self) -> display::T {
        let mut layout_data_ref = self.mutate_layout_data();
        let node_layout_data_wrapper = layout_data_ref.as_mut().unwrap();
        let style = node_layout_data_wrapper.data.after_style.as_ref().unwrap();
        style.get_box().display
    }

    #[inline]
    fn has_before_pseudo(&self) -> bool {
        let layout_data_wrapper = self.borrow_layout_data();
        let layout_data_wrapper_ref = layout_data_wrapper.as_ref().unwrap();
        layout_data_wrapper_ref.data.before_style.is_some()
    }

    #[inline]
    fn has_after_pseudo(&self) -> bool {
        let layout_data_wrapper = self.borrow_layout_data();
        let layout_data_wrapper_ref = layout_data_wrapper.as_ref().unwrap();
        layout_data_wrapper_ref.data.after_style.is_some()
    }

    /// Returns an iterator over this node's children.
    fn children(&self) -> ThreadSafeLayoutNodeChildrenIterator<'ln> {
        ThreadSafeLayoutNodeChildrenIterator {
            current_node: self.first_child(),
            parent_node: Some(self.clone()),
        }
    }

    /// Traverses the tree in postorder.
    ///
    /// TODO(pcwalton): Offer a parallel version with a compatible API.
    fn traverse_postorder_mut<T:PostorderNodeMutTraversal>(&mut self, traversal: &mut T)
                                  -> bool {
        if traversal.should_prune(self) {
            return true
        }

        let mut opt_kid = self.first_child();
        loop {
            match opt_kid {
                None => break,
                Some(mut kid) => {
                    if !kid.traverse_postorder_mut(traversal) {
                        return false
                    }
                    unsafe {
                        opt_kid = kid.next_sibling()
                    }
                }
            }
        }

        traversal.process(self)
    }

    fn is_ignorable_whitespace(&self) -> bool {
        unsafe {
            let text: LayoutJS<Text> = match TextCast::to_layout_js(self.get_jsmanaged()) {
                Some(text) => text,
                None => return false
            };

            if !is_whitespace(CharacterDataCast::from_layout_js(&text).data_for_layout()) {
                return false
            }

            // NB: See the rules for `white-space` here:
            //
            //    http://www.w3.org/TR/CSS21/text.html#propdef-white-space
            //
            // If you implement other values for this property, you will almost certainly
            // want to update this check.
            match self.style().get_inheritedtext().white_space {
                white_space::T::normal => true,
                _ => false,
            }
        }
    }

}

impl<'ln> TLayoutNode2<'ln> for ThreadSafeLayoutNode<'ln> {
    fn first_child(&self) -> Option<ThreadSafeLayoutNode<'ln>> {
        if self.pseudo != PseudoElementType::Normal {
            return None
        }

        if self.has_before_pseudo() {
            // FIXME(pcwalton): This logic looks weird. Is it right?
            match self.pseudo {
                PseudoElementType::Normal => {
                    let pseudo_before_node = self.with_pseudo(PseudoElementType::Before(self.get_before_display()));
                    return Some(pseudo_before_node)
                }
                PseudoElementType::Before(display::T::inline) => {}
                PseudoElementType::Before(_) => {
                    let pseudo_before_node = self.with_pseudo(PseudoElementType::Before(display::T::inline));
                    return Some(pseudo_before_node)
                }
                _ => {}
            }
        }

        unsafe {
            self.get_jsmanaged().first_child_ref().map(|node| self.new_with_this_lifetime(&node))
        }
    }

    fn text_content(&self) -> Vec<ContentItem> {
        if self.pseudo != PseudoElementType::Normal {
            let layout_data_ref = self.borrow_layout_data();
            let node_layout_data_wrapper = layout_data_ref.as_ref().unwrap();

            if self.pseudo.is_before() {
                let before_style = node_layout_data_wrapper.data.before_style.as_ref().unwrap();
                return get_content(&before_style.get_box().content)
            } else {
                let after_style = node_layout_data_wrapper.data.after_style.as_ref().unwrap();
                return get_content(&after_style.get_box().content)
            }
        }
        self.node.text_content()
    }
}

pub struct ThreadSafeLayoutNodeChildrenIterator<'a> {
    current_node: Option<ThreadSafeLayoutNode<'a>>,
    parent_node: Option<ThreadSafeLayoutNode<'a>>,
}

impl<'a> Iterator for ThreadSafeLayoutNodeChildrenIterator<'a> {
    type Item = ThreadSafeLayoutNode<'a>;
    fn next(&mut self) -> Option<ThreadSafeLayoutNode<'a>> {
        let node = self.current_node.clone();

        match node {
            Some(ref node) => {
                if node.pseudo.is_after() {
                    return None
                }

                match self.parent_node {
                    Some(ref parent_node) => {
                        if parent_node.pseudo == PseudoElementType::Normal {
                            self.current_node = self.current_node.clone().and_then(|node| {
                                unsafe {
                                    node.next_sibling()
                                }
                            });
                        } else {
                            self.current_node = None;
                        }
                    }
                    None => {}
                }
            }
            None => {
                match self.parent_node {
                    Some(ref parent_node) => {
                        if parent_node.has_after_pseudo() {
                            let pseudo_after_node = if parent_node.pseudo == PseudoElementType::Normal {
                                let pseudo = PseudoElementType::After(parent_node.get_after_display());
                                Some(parent_node.with_pseudo(pseudo))
                            } else {
                                None
                            };
                            self.current_node = pseudo_after_node;
                            return self.current_node.clone()
                        }
                   }
                   None => {}
                }
            }
        }

        node
    }
}

