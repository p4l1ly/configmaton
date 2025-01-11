struct Node<T> {
    value: T,
    next: Option<Box<Node<T>>>,
}

pub struct Holder<T> {
    head: Option<Box<Node<T>>>,
}

impl<T> Holder<T> {
    pub fn new() -> Self {
        Holder { head: None }
    }

    pub fn add(&mut self, value: T) -> *mut T {
        let old = self.head.take();
        self.head = Some(Box::new(Node { value, next: old }));
        &mut self.head.as_mut().unwrap().value
    }

    pub fn iter_mut(&mut self) -> Iter<T> {
        Iter { cur: self.head.as_mut().map(|node| &mut **node as *mut _) }
    }

    pub fn clear(&mut self) {
        self.head = None;
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }
}

pub struct Iter<T> {
    cur: Option<*mut Node<T>>,
}

impl<T> Iterator for Iter<T> {
    type Item = *mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(node) = self.cur {
            let node = unsafe { &mut *node };
            self.cur = node.next.as_mut().map(|node| &mut **node as *mut _);
            Some(&mut node.value)
        } else {
            None
        }
    }
}
