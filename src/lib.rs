use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

/// Joque implements a lock-free double-ended queue.
#[allow(dead_code)]
struct Joque<T> {
    // Contains an op_id muxed with a "pointer" into backing
    deque: [AtomicUsize; 50],
    pub left: AtomicUsize,  // left side of deque
    right: AtomicUsize, // right side of deque
    capacity: usize, // size of heap
    
    // 💡✨: 128 entry chunks, with 128/bitwise find empty, NO GC. 
    backing: [AtomicPtr<T>; 100], // zero is the null ptr in this reference frame
    op_id: AtomicU32,               
    idx: AtomicU32,
}
#[allow(dead_code)]
impl<T> Joque<T> {
    pub fn new() -> Self {
        Joque {
            deque: [const { AtomicUsize::new(0) }; 50], // TODO: 💀 dynamically resizable
            left: AtomicUsize::new(25),
            right: AtomicUsize::new(26),
            capacity: 50,
            backing: [const { AtomicPtr::new(std::ptr::null_mut()) }; 100], // TODO: 💀 dynamically resizable
            op_id: AtomicU32::new(0),
            idx: AtomicU32::new(1),
        }
    }
    pub fn push_front(&self, item: Box<T>) {
        let back_idx = self.idx.fetch_add(1, Ordering::SeqCst); // TODO: 💀 after 400 write/read cycles
        self.backing[back_idx as usize].store(Box::into_raw(item), Ordering::SeqCst);
        loop {
            let this_left = self.left.load(Ordering::SeqCst);
            let lval = self.deque[this_left].load(Ordering::SeqCst);
            let entry = ((self.op_id.fetch_add(1, Ordering::SeqCst) as usize) << 32) | back_idx as usize;
            // break deque's neck to point at our crap
            if self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                // println!("Dropped into {back_idx}, ok");
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
            if let Ok(_) = self.deque[this_left].compare_exchange(lval, entry, Ordering::SeqCst, Ordering::SeqCst) {
                // println!("Seeking from {}, ok", lval & 0xFFFFFFFF);
                let out = self.backing[(lval & 0xFFFFFFFF) as usize].swap(std::ptr::null_mut(), Ordering::SeqCst );
                self.left.fetch_add(1, Ordering::SeqCst);
                unsafe { 
                    return Box::from_raw(out);
                }
            }
        }
    }

    pub fn get(&self, _index: usize) -> Option<usize> {
        None
    }

    pub fn mutate<F>(&self, _index: usize, _op: F) 
        where F: FnMut(T), {

    }

    pub fn set(&self, _index: usize, _val: usize) {

    }

    pub fn get_unchecked(&self, _index: usize) -> usize {
        unimplemented!()
    }

    pub fn borrow(&self) -> &Self {
        &self
    }
}


mod tests {
    use std::sync::atomic::Ordering;
    use crate::Joque;

    #[test]
    pub fn basic_test() {
        let deque = Joque::new();

        deque.push_front(Box::new("squirpy"));
        deque.push_front(Box::new("squirp"));
        deque.push_front(Box::new("squirp"));

        assert_eq!("squirp", *deque.pop_front());
        assert_eq!("squirp", *deque.pop_front());
        assert_eq!("squirpy", *deque.pop_front());
    }

    use loom::sync::Arc;
    use loom::sync::atomic::AtomicUsize;
    use loom::sync::atomic::Ordering::{Acquire, Release, Relaxed};
    use loom::thread;
    
    #[test]
    fn permute_interleaved_modification() {
        let THREAD_COUNT = 4;
        loom::model(move || {
            let deque = Arc::new(Joque::new());
            
            let ths: Vec<_> = (0..THREAD_COUNT).map(|idx| {
                let big_deque = deque.clone();
                thread::spawn( move || {
                    big_deque.push_front(Box::new(idx));
                    big_deque.pop_front();
                    big_deque.push_front(Box::new(idx+1));
                    big_deque.push_front(Box::new(idx+2));
                })})
                .collect();
            // 8 * .

            for th in ths {
                th.join().unwrap();
            }

            assert_eq!(25 - THREAD_COUNT*4, deque.clone().left.load(Ordering::Relaxed));
        });
    }

    #[test]
    #[should_panic]
    fn buggy_concurrent_inc() {
        loom::model(|| {
            let num = Arc::new(AtomicUsize::new(0));

            let ths: Vec<_> = (0..2)
                .map(|_| {
                    let num = num.clone();
                    thread::spawn(move || {
                        let curr = num.load(Acquire);
                        num.store(curr + 1, Release);
                    })
                })
                .collect();

            for th in ths {
                th.join().unwrap();
            }

            assert_eq!(2, num.load(Relaxed));
        });
    }
}