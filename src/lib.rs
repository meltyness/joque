use std::{collections::HashMap, sync::atomic::{AtomicU32, AtomicUsize, Ordering}};

/// Joque implements a lock-free double-ended queue.
struct Joque<T> {
    // Contains an op_id muxed with a "pointer" into backing
    deque: [AtomicUsize; 100],
    left: AtomicUsize,  // left side of deque
    right: AtomicUsize, // right side of deque
    capacity: usize, // size of heap

    backing: HashMap<u32, T>,
    op_id: AtomicU32,
    idx: AtomicU32,
}

impl<T> Joque<T> {
    pub fn new() -> Self {
        Joque {
            deque: [const { AtomicUsize::new(0) }; 100],
            left: AtomicUsize::new(50),
            right: AtomicUsize::new(51),
            capacity: 100,
            backing: HashMap::new(),
            op_id: AtomicU32::new(0),
            idx: AtomicU32::new(0),
        }
    }
    pub fn push_front(&mut self, item: T) {
        loop {
            let this_left = self.left.load(Ordering::SeqCst);
            let lval = self.deque[this_left].load(Ordering::SeqCst);
            self.op_id.fetch_add(1, Ordering::SeqCst);
            let idx = (self.idx.load(Ordering::SeqCst) as usize);
            let entry = ((self.op_id.load(Ordering::SeqCst) as usize) << 32) | idx;
            if self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                self.backing.insert(idx as u32, item);
                self.idx.fetch_add(1, Ordering::SeqCst);
                self.left.fetch_sub(1, Ordering::SeqCst);
                break;
            }
        }
    }

    pub fn pop_front(&mut self) -> T {
        loop {
            let this_left = self.left.load(Ordering::SeqCst);
            let lval = self.deque[this_left].load(Ordering::SeqCst);
            self.op_id.fetch_sub(1, Ordering::SeqCst);
            let idx = (self.idx.load(Ordering::SeqCst) as usize);
            let entry = ((self.op_id.load(Ordering::SeqCst) as usize) << 32) | idx;
            if let Ok(val) = self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst) {
                let out = self.backing.remove(&((val & 0xFFFFFFFF) as u32)).unwrap();
                self.idx.fetch_sub(1, Ordering::SeqCst);
                self.left.fetch_add(1, Ordering::SeqCst);
                return out;
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

        deque.push_front("squirp");

        assert_eq!("squirp", deque.pop_front());
    }
}