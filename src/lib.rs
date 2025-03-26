use std::{collections::HashMap, sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering}};

/// Joque implements a lock-free double-ended queue.
struct Joque<T> {
    // Contains an op_id muxed with a "pointer" into backing
    deque: [AtomicUsize; 100],
    left: AtomicUsize,  // left side of deque
    right: AtomicUsize, // right side of deque
    capacity: usize, // size of heap
    
    // 💡✨: 128 entry chunks, with 128/bitwise find empty, NO GC. 
    backing: [AtomicPtr<T>; 400], // zero is the null ptr in this reference frame
    op_id: AtomicU32,               
    idx: AtomicU32,
}

impl<T> Joque<T> {
    pub fn new() -> Self {
        Joque {
            deque: [const { AtomicUsize::new(0) }; 100], // TODO: 💀 dynamically resizable
            left: AtomicUsize::new(50),
            right: AtomicUsize::new(51),
            capacity: 100,
            backing: [const { AtomicPtr::new(std::ptr::null_mut()) }; 400], // TODO: 💀 dynamically resizable
            op_id: AtomicU32::new(0),
            idx: AtomicU32::new(1),
        }
    }
    pub fn push_front(&self, mut item: Box<T>) {
        let back_idx = self.idx.fetch_add(1, Ordering::SeqCst); // TODO: 💀 after 400 write/read cycles
        self.backing[back_idx as usize].store(Box::into_raw(item), Ordering::SeqCst);
        loop {
            let this_left = self.left.load(Ordering::SeqCst);
            let lval = self.deque[this_left].load(Ordering::SeqCst);
            let entry = ((self.op_id.fetch_add(1, Ordering::SeqCst) as usize) << 32) | back_idx as usize;
            // break deque's neck to point at our crap
            if self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                println!("Dropped into {back_idx}, ok");
                self.left.fetch_sub(1, Ordering::SeqCst);
                break;
            }
        }
    }

    pub fn pop_front(&self) -> Box<T> {
        loop {
            let this_left = self.left.load(Ordering::SeqCst) + 1;
            let lval = self.deque[this_left].load(Ordering::SeqCst);
            let idx = 0;
            let entry = ((self.op_id.fetch_add(1, Ordering::SeqCst) as usize) << 32) | idx;
            if let Ok(val) = self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst) {
                println!("Seeking from {}, ok", lval & 0xFFFFFFFF);
                let out = self.backing[(lval & 0xFFFFFFFF) as usize].swap(std::ptr::null_mut(), Ordering::SeqCst );
                self.left.fetch_add(1, Ordering::SeqCst);
                unsafe { 
                    return Box::from_raw(out);
                }
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<usize> {
        None
    }

    pub fn mutate<F>(&self, index: usize, op: F) 
        where F: FnMut(T), {

    }

    pub fn set(&self, index: usize, val: usize) {

    }

    pub fn get_unchecked(&self, index: usize) -> usize {
        unimplemented!()
    }
}


mod tests {
    use crate::Joque;

    #[test]
    pub fn basic_test() {
        let mut deque = Joque::new();

        deque.push_front(Box::new("squirpy"));
        deque.push_front(Box::new("squirp"));
        deque.push_front(Box::new("squirp"));

        assert_eq!("squirp", *deque.pop_front());
        assert_eq!("squirp", *deque.pop_front());
        assert_eq!("squirpy", *deque.pop_front());
    }
}