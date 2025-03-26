use std::{sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering}, thread};

/// Joque implements a lock-free double-ended queue.
#[allow(dead_code)]
struct Joque<T> {
    // Contains an op_id muxed with a "pointer" into backing
    deque: Vec<AtomicUsize>,
    leftright: AtomicUsize,
    capacity: usize, // size of heap
    
    // 💡✨: 128 entry chunks, with 128/bitwise find empty, NO GC. 
    backing: Vec<AtomicPtr<T>>, // zero is the null ptr in this reference frame
    op_id: AtomicU32,               
    idx: AtomicU32,
}

const LEFTMASK: usize = 0x00000000_FFFFFFFF;
const RIGHTMASK: usize = 0xFFFFFFFF_00000000;
const ONE: usize = 0x00000001_00000000;

#[allow(dead_code)]
impl<T> Joque<T> {
    pub fn new(width: usize) -> Self {
        if width < 10 { panic! ("let's not"); }
        let left = width / 2;
        let right = (left + 1) << 32;
        Joque {
            deque: std::iter::from_fn(|| Some(AtomicUsize::new(0))).take(width).collect(), // TODO: 💀 dynamically resizable
            leftright: AtomicUsize::new(left | right),
            capacity: width,
            backing: std::iter::from_fn(|| Some(AtomicPtr::new(std::ptr::null_mut()))).take(width*4).collect(), // TODO: 💀 dynamically resizable
            op_id: AtomicU32::new(0),
            idx: AtomicU32::new(1),
        }
    }

    pub fn push_front(&self, item: Box<T>) {
        // reserve backing storage
        //  - unique until wrapped
        let backing_idx = self.idx.fetch_add(1, Ordering::Acquire); // TODO: 💀 after 400 write/read cycles

        // immediately write deposited value into backing storage
        //  - no fallible push
        self.backing[backing_idx as usize].store(Box::into_raw(item), Ordering::SeqCst);

        // for each CAS retry
        loop {
            // fetch the current target index
            let this_left = self.fetch_extent_acq().0;

            // fetch the associated backing location
            let lval = self.deque[this_left].load(Ordering::Acquire);

            // increment and fetch op id, mux with backing_idx
            let entry = ((self.op_id.fetch_add(1, Ordering::Acquire) as usize) << 32) | backing_idx as usize;

            // break deque's neck to point at our crap
            if self.deque[this_left].compare_exchange(lval, entry, Ordering::Acquire, Ordering::Acquire).is_ok() {
                
                // println!("Pushed onto {}, ok", entry & LEFTMASK);

                // we know that we succeeded, but this isn't necessarily synchronized
                self.leftright.fetch_sub(1, Ordering::Acquire);
                break;
            }
        }
    }

    pub fn pop_front(&self) -> Option<Box<T>> {
        loop {
            let (sens_left, sens_right) = self.fetch_extent_rel();
            if sens_left + 1 >= sens_right { return None; }
            let this_left = sens_left + 1;
            let lval = self.deque[this_left].load(Ordering::Acquire);
            if lval & LEFTMASK == 0 { thread::yield_now(); return None; }
            let idx = 0;
            let entry = ((self.op_id.fetch_add(1, Ordering::Release) as usize) << 32) | idx;
            if let Ok(old_one) = self.deque[this_left].compare_exchange(lval, entry, Ordering::Release, Ordering::Relaxed) {
                // println!("Seeking from {}, ok", lval & LEFTMASK);
                let out = self.backing[(lval & LEFTMASK) as usize].swap(std::ptr::null_mut(), Ordering::Release );
                self.leftright.fetch_add(1, Ordering::Release);
                unsafe { 
                    if out.is_null() {
                        println!("Well that was weird I couldn't pull {}", lval & LEFTMASK);
                        println!("Here's some other stuff: ");
                        println!("Directed pop state: {this_left}");
                        println!("Claimed Op ID: {}", entry >> 32);
                        println!("Old Cmp Data: {} {}", old_one & RIGHTMASK >> 32, old_one & LEFTMASK);
                        return None;
                    } else {
                        return Some(Box::from_raw(out));
                    }
                }
            }
        }
    }

    fn fetch_extent_acq(&self) -> (usize, usize) {
        let muxed = self.leftright.load(Ordering::Acquire);
        let left_demuxed = muxed & LEFTMASK;
        let right_demuxed = (muxed & RIGHTMASK) >> 32;
        (left_demuxed, right_demuxed)
    }

    fn fetch_extent_rel(&self) -> (usize, usize) {
        let muxed = self.leftright.load(Ordering::Relaxed);
        let left_demuxed = muxed & LEFTMASK;
        let right_demuxed = (muxed & RIGHTMASK) >> 32;
        (left_demuxed, right_demuxed)
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
    use crate::{Joque, LEFTMASK};

    #[test]
    pub fn basic_test() {
        let deque = Joque::new(25);

        deque.push_front(Box::new("squirpy"));
        deque.push_front(Box::new("squirp"));
        deque.push_front(Box::new("squirp"));

        assert_eq!("squirp", *deque.pop_front().unwrap());
        assert_eq!("squirp", *deque.pop_front().unwrap());
        assert_eq!("squirpy", *deque.pop_front().unwrap());
    }

    use loom::sync::Arc;
    use loom::sync::atomic::AtomicUsize;
    use loom::sync::atomic::Ordering::{Acquire, Release, Relaxed};
    use loom::thread;
    
    #[test]
    fn permute_interleaved_modification() {
        let THREAD_COUNT = 4;
        let WIDTH = 25;
        let LEFT_START = WIDTH / 2;
        loom::model(move || {
            let deque = Arc::new(Joque::new(WIDTH));
            
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

            assert_eq!(LEFT_START - THREAD_COUNT*2, deque.clone().leftright.load(Ordering::Relaxed) & LEFTMASK);
        });
    }

    #[test]
    fn interleaved_modification() {
        let THREAD_COUNT = 64;
        let PAD_WIDTH = 16;
        let WIDTH = 512;
        let LEFT_START = WIDTH / 2;
        let RERUNS = 10000;
        
        for rerun in 0..RERUNS {
            println!("~~~~~ {rerun} ~~~~~");
            let deque = std::sync::Arc::new(Joque::new(WIDTH));

            for _ in 0..PAD_WIDTH {
                deque.push_front(Box::new(usize::MAX));
            }

            let mut ths: Vec<_> = (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();

                std::thread::spawn( move || {
                    big_deque.push_front(Box::new(idx));
                    while big_deque.pop_front().is_none() { }
                    big_deque.push_front(Box::new(idx+1));
                    big_deque.push_front(Box::new(idx+2));
                })})
                .collect();

            ths.append(&mut (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();

                std::thread::spawn( move || {
                    big_deque.push_front(Box::new(idx));
                    big_deque.push_front(Box::new(idx+1));
                    while big_deque.pop_front().is_none() { }
                    big_deque.push_front(Box::new(idx+2));
                })})
                .collect());

            for th in ths {
                th.join().unwrap();
            }

            assert_eq!(LEFT_START - THREAD_COUNT*2 - PAD_WIDTH, deque.clone().leftright.load(Ordering::Relaxed) & LEFTMASK);
        }
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