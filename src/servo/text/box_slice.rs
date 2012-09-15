pub struct BoxSlice {
    priv full_text: @~str,
    priv slice_: (uint, uint),
}

pub fn BoxSlice(s: @~str) -> BoxSlice {
    BoxSlice {
        full_text: s,
        slice_: (0, s.len())
    }
}

impl BoxSlice {
    fn slice(begin: uint, end: uint) -> BoxSlice {
        assert begin <= end;
        assert end <= self.slice_.second() - self.slice_.first();
        // Pick up other slicing assertions (about char boundaries in particular)
        str::view(*self.full_text,
                  self.slice_.first() + begin,
                  self.slice_.first() + end);
                  
        BoxSlice {
            full_text: self.full_text,
            slice_: (self.slice_.first() + begin,
                     self.slice_.first() + end)
        }
    }

    fn len() -> uint { self.slice_.second() - self.slice_.first() }

    fn trim_left() -> BoxSlice {
        match self.find(|c| !char::is_whitespace(c)) {
            Some(idx) => self.slice(idx, self.len()),
            None => self.slice(self.len(), self.len())
        }
    }

    fn borrow(&self) -> &str {
        let local_slice = str::view(*self.full_text, self.slice_.first(), self.slice_.second());
        // FIXME: How do I do this safely?
        let parent_slice = unsafe { unsafe::reinterpret_cast(&local_slice) };
        return parent_slice;
    }

    fn extend_through(other: BoxSlice) -> BoxSlice {
        assert self.slice_.first() <= other.slice_.first();
        assert self.slice_.second() <= other.slice_.second();
        BoxSlice {
            full_text: self.full_text,
            slice_: (self.slice_.first(), other.slice_.second()),
        }
    }

    fn find(f: fn(char) -> bool) -> Option<uint> { str::find(self.borrow(), f) }

    fn is_not_empty() -> bool { self.borrow().is_not_empty() }

    fn to_str() -> ~str { self.borrow().to_str() }
}
