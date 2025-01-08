use std::{cell::RefCell, marker::PhantomData};

pub struct TimedRefCell<'a, T> {
    value: RefCell<T>,
    _phantom: PhantomData<&'a T>,
}

impl<'a, T> TimedRefCell<'a, T> {
    pub fn borrow(&self) -> std::cell::Ref<'_, T> {
        self.value.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, T> {
        self.value.borrow_mut()
    }
}

struct Node<'a, T> {
    value: TimedRefCell<'a, T>,
    next: Option<Box<Node<'a, T>>>,
}

pub struct Holder<'a, T> {
    head: RefCell<Option<Box<Node<'a, T>>>>,
    _phantom: PhantomData<&'a ()>
}

impl<'a, T> Holder<'a, T> {
    pub fn new() -> Self {
        Holder { head: RefCell::new(None), _phantom: PhantomData }
    }

    pub fn add(&self, value: T) -> &TimedRefCell<'a, T> {
        let old = self.head.replace(
            Some(Box::new(Node {
                value: TimedRefCell { value: RefCell::new(value), _phantom: PhantomData },
                next: None,
            }))
        );
        let mut head_borrow = self.head.borrow_mut();
        let new = head_borrow.as_mut().unwrap();
        new.next = old;

        unsafe { &*(&new.value as *const _) }
    }
}

// impl<'a, T> Drop for Holder<'a, T> {
//     fn drop(&mut self) {
//         println!("Dropping holder");
//     }
// }

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_works() {
        let container = Holder::new();

        let item1 = container.add(1);
        let item2 = container.add(2);

        println!("{}", item1.borrow()); // 1
        println!("{}", item2.borrow()); // 2

        *item1.borrow_mut() = 10;

        // Can add more while holding references
        let item3 = container.add(3);

        println!("{}", item1.borrow()); // 10
        println!("{}", item2.borrow()); // 2
        println!("{}", item3.borrow()); // 3
    }

    #[test]
    fn it_does_not_build() {
        {
            println!("block start");
            let container = Holder::new();
            let item = container.add(3);
            println!("borrow1 {}", item.borrow());
            println!("block end");
            item
        };
        println!("after block end");
        // This should not compile, TODO use trybuild
        // println!("borrow2 {}", item.borrow());
        println!("after borrow");
    }
}
