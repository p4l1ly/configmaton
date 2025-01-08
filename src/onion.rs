use hashbrown::HashMap;
use crate::holder::Holder;

pub struct Onion<'a> {
    parent: Option<*const Self>,
    children: Holder<Self>,
    data: HashMap<&'a [u8], &'a [u8]>,
}

impl<'a> Onion<'a>
{
    pub fn new() -> Self {
        Onion {
            parent: None,
            children: Holder::new(),
            data: HashMap::new(),
        }
    }

    pub fn make_child(&mut self) -> &mut Self {
        self.children.add(Onion {
            parent: Some(self),
            children: Holder::new(),
            data: HashMap::new(),
        })
    }

    pub fn get_rec(&self, key: &[u8]) -> Option<&'a [u8]> {
        if let Some(value) = self.data.get(key) {
            return Some(value);
        }
        unsafe { &*(self.parent?) }.get_rec(key)
    }

    pub fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
        if let Some(value) = self.data.get(key) {
            return Some(value);
        }

        let mut parent = self.parent?;
        loop {
            let parent_onion = unsafe { &*parent };
            if let Some(value) = parent_onion.data.get(key) {
                return Some(value);
            }
            parent = parent_onion.parent?;
        }
    }

    pub fn set(&mut self, key: &'a [u8], value: &'a [u8]) {
        self.data.insert(key, value);
    }
}
