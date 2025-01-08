struct Node<T> {
    value: T,
    _next: Option<Box<Node<T>>>,
}

pub struct Holder<T> {
    head: Option<Box<Node<T>>>,
}

impl<T> Holder<T> {
    pub fn new() -> Self {
        Holder { head: None }
    }

    pub fn add(&mut self, value: T) -> &mut T {
        let mslf = self;
        let old = mslf.head.take();
        mslf.head = Some(Box::new(Node { value, _next: old }));
        &mut mslf.head.as_mut().unwrap().value
    }
}
