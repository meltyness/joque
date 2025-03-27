#![allow(non_snake_case)]
use std::{sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering}, thread};

/// Joque implements a lock-free double-ended queue.
#[allow(dead_code)]
struct Joque<T> {
    // Contains an op_id muxed with a "pointer" into backing
    deque: Vec<AtomicUsize>,
    leftright: AtomicUsize,
    capacity: u32, // size of heap
    
    // 💡✨: 128 entry chunks, with 128/bitwise find empty, NO GC. 
    // 💡✨: multiple reclamation stacks, thread across them when doing reclamation
    backing: Vec<AtomicPtr<T>>, // zero is the null ptr in this reference frame
    op_id: AtomicU32,               
    idx: AtomicU32,
}

const LEFTMASK: usize = 0x00000000_FFFFFFFF;
const RIGHTMASK: usize = 0xFFFFFFFF_00000000;
const ONE: usize = 0x00000001_00000000;

#[allow(dead_code)]
impl<T> Joque<T> {
    pub fn new(width: u32) -> Self {
        if width < 10 { panic! ("let's not"); }
        let left = width / 2;
        let right = (left as usize) << 32;
        Joque {
            deque: std::iter::from_fn(|| Some(AtomicUsize::new(0))).take(width as usize).collect(), // TODO: 💀 dynamically resizable
            leftright: AtomicUsize::new(left as usize | right),
            capacity: width,
            backing: std::iter::from_fn(|| Some(AtomicPtr::new(std::ptr::null_mut()))).take((width*4) as usize).collect(), // TODO: 💀 dynamically resizable
            op_id: AtomicU32::new(0),
            idx: AtomicU32::new(1),
        }
    }

    pub fn push_front(&self, item: Box<T>) {
        let backing_idx = self.idx.fetch_add(1, Ordering::Acquire); // TODO: 💀 after 400 write/read cycles
        self.backing[backing_idx as usize].store(Box::into_raw(item), Ordering::SeqCst);
        loop {
            let this_left = self.fetch_extent_acq().0;
            let lval = self.deque[(this_left % self.capacity) as usize].load(Ordering::Acquire);
            let entry = ((self.op_id.fetch_add(1, Ordering::Acquire) as usize) << 32) | backing_idx as usize;
            if self.deque[(this_left % self.capacity) as usize].compare_exchange(lval, entry, Ordering::Acquire, Ordering::Acquire).is_ok() {
                self.leftright.fetch_sub(1, Ordering::Acquire);
                break;
            }
        }
    }

    pub fn pop_front(&self) -> Option<Box<T>> {
        loop {
            let (sens_left, sens_right) = self.fetch_extent_rel();
            if sens_left % self.capacity == sens_right % self.capacity { return None; }
            let this_left = sens_left + 1;
            let lval = self.deque[(this_left % self.capacity) as usize].load(Ordering::Acquire);
            if lval & LEFTMASK == 0 { thread::yield_now(); return None; }
            let idx = 0;
            let entry = ((self.op_id.fetch_add(1, Ordering::Release) as usize) << 32) | idx;
            if let Ok(_old_one) = self.deque[(this_left % self.capacity) as usize]
                                            .compare_exchange(lval, entry, Ordering::Release, Ordering::Relaxed) {
                // println!("Seeking from {}, ok", lval & LEFTMASK);
                let out = self.backing[(lval & LEFTMASK) as usize].swap(std::ptr::null_mut(), Ordering::Release );
                self.leftright.fetch_add(1, Ordering::Release);
                unsafe { 
                    if out.is_null() {
                        // println!("Well that was weird I couldn't pull {}", lval & LEFTMASK);
                        // println!("Here's some other stuff: ");
                        // println!("Directed pop state: {this_left}");
                        // println!("Claimed Op ID: {}", entry >> 32);
                        // println!("Old Cmp Data: {} {}", old_one & RIGHTMASK >> 32, old_one & LEFTMASK);
                        panic!("This should not occur");
                        //return None;
                    } else {
                        return Some(Box::from_raw(out));
                    }
                }
            }
        }
    }

    pub fn push_back(&self, item: Box<T>) {
        // reserve backing storage
        //  - unique until wrapped
        let backing_idx = self.idx.fetch_add(1, Ordering::Acquire); // TODO: 💀 after 400 write/read cycles
        self.backing[backing_idx as usize].store(Box::into_raw(item), Ordering::SeqCst);
        loop {
            let this_right = self.fetch_extent_acq().1;
            let rval = self.deque[(this_right % self.capacity) as usize].load(Ordering::Acquire);
            let entry = ((self.op_id.fetch_add(1, Ordering::Acquire) as usize) << 32) | backing_idx as usize;
            if self.deque[(this_right % self.capacity) as usize].compare_exchange(rval, entry, Ordering::Acquire, Ordering::Acquire).is_ok() {
                self.leftright.fetch_add(ONE, Ordering::Acquire); // notice using ONE; a shifted value for the halfreg
                break;
            }
        }
    }

    pub fn pop_back(&self) -> Option<Box<T>> {
        loop {
            let (sens_left, sens_right) = self.fetch_extent_rel();
            if sens_left % self.capacity == sens_right % self.capacity { return None; }
            let this_right = sens_right.wrapping_sub(1);
            let rval = self.deque[(this_right % self.capacity) as usize].load(Ordering::Acquire);
            if rval & LEFTMASK == 0 { thread::yield_now(); return None; }
            let idx = 0;
            let entry = ((self.op_id.fetch_add(1, Ordering::Release) as usize) << 32) | idx;
            if let Ok(_old_one) = self.deque[(this_right % self.capacity) as usize]
                                            .compare_exchange(rval, entry, Ordering::Release, Ordering::Relaxed) {
                // println!("Seeking from {}, ok", lval & LEFTMASK);
                let out = self.backing[(rval & LEFTMASK) as usize].swap(std::ptr::null_mut(), Ordering::Release );
                self.leftright.fetch_sub(ONE, Ordering::Release); // notice using ONE here
                unsafe { 
                    if out.is_null() {
                        // println!("Well that was weird I couldn't pull {}", rval & RIGHTMASK);
                        // println!("Here's some other stuff: ");
                        // println!("Directed pop state: {this_right}");
                        // println!("Claimed Op ID: {}", entry >> 32);
                        // println!("Old Cmp Data: {} {}", old_one & RIGHTMASK >> 32, old_one & LEFTMASK);
                        panic!("This should not occur");
                        //return None;
                    } else {
                        return Some(Box::from_raw(out));
                    }
                }
            }
        }
    }

    fn fetch_extent_acq(&self) -> (u32, u32) {
        let muxed = self.leftright.load(Ordering::Acquire);
        let left_demuxed = muxed & LEFTMASK;
        let right_demuxed = (muxed & RIGHTMASK) >> 32;
        (left_demuxed as u32, right_demuxed as u32)
    }

    fn fetch_extent_rel(&self) -> (u32, u32) {
        let muxed = self.leftright.load(Ordering::Relaxed);
        let left_demuxed = muxed & LEFTMASK;
        let right_demuxed = (muxed & RIGHTMASK) >> 32;
        (left_demuxed as u32, right_demuxed as u32)
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
    #[allow(unused_imports)]
    use std::sync::atomic::Ordering;
    #[allow(unused_imports)]
    use crate::{Joque, LEFTMASK, RIGHTMASK};

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

    #[test]
    pub fn basic_wrap() {
        let deque = Joque::new(25);

        for _i in 0..49 {
            // print!("{i}");
            deque.push_front(Box::new("oogah"));
            deque.pop_back();
            
            deque.push_front(Box::new("boogah"));
            deque.pop_back();
        }
    }

    #[allow(unused_imports)]
    use loom::sync::Arc;
    #[allow(unused_imports)]
    use loom::sync::atomic::AtomicUsize;
    #[allow(unused_imports)]
    use loom::sync::atomic::Ordering::{Acquire, Release, Relaxed};
    #[allow(unused_imports)]
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

            assert_eq!(LEFT_START - THREAD_COUNT*2, (deque.clone().leftright.load(Ordering::Relaxed) & LEFTMASK) as u32);
        });
    }

    #[test]
    fn interleaved_modification() {
        
        // println!("trace");
        let THREAD_COUNT = 32u32;
        let PAD_WIDTH = 0u32;
        let WIDTH = 4096;
        let LEFT_START = WIDTH / 2;
        let RERUNS = 100;
        
        for _rerun in 0..RERUNS {
            // println!("~~~~~ {rerun} ~~~~~");
            let deque = std::sync::Arc::new(Joque::new(WIDTH));

            for _ in 0..PAD_WIDTH {
                deque.push_front(Box::new(u32::MAX));
            }

            let mut ths: Vec<_> = (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();
                
                std::thread::spawn( move || {
                    big_deque.push_front(Box::new(idx));
                    let _ = big_deque.pop_front().is_none();
                    big_deque.push_front(Box::new(idx+1));
                    big_deque.push_front(Box::new(idx+2));
                })})
                .collect();

            ths.append(&mut (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();

                std::thread::spawn( move || {
                    big_deque.push_front(Box::new(idx));
                    big_deque.push_front(Box::new(idx+1));
                    let _ = big_deque.pop_front().is_none();
                    big_deque.push_front(Box::new(idx+2));
                })})
                .collect());

            for th in ths {
                th.join().unwrap();
            }
            // despite the fact that each thread should contribute net +1 push into the listing,
            // there's a stochastic event, which may occur, such that all threads behaving this way simultaneously
            // observe and empty stack when popping, and so there's a chance of 'failed' pops.
            assert!(LEFT_START - THREAD_COUNT*2 - PAD_WIDTH >= (deque.clone().leftright.load(Ordering::Relaxed) & LEFTMASK) as u32);
        }
    }

    #[test]
    fn interleaved_right_modification() {
        
        // println!("trace");
        let THREAD_COUNT = 32u32;
        let PAD_WIDTH = 0;
        let WIDTH = 4096;
        let LEFT_START = WIDTH / 2;
        let RIGHT_START = LEFT_START;
        let RERUNS = 100;
        
        for _rerun in 0..RERUNS {
            // println!("~~~~~ {rerun} ~~~~~");
            let deque = std::sync::Arc::new(Joque::new(WIDTH));

            for _ in 0..PAD_WIDTH {
                deque.push_back(Box::new(u32::MAX));
            }

            let mut ths: Vec<_> = (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();
                
                std::thread::spawn( move || {
                    big_deque.push_back(Box::new(idx));
                    let out = big_deque.pop_back().is_none() as i32;
                    big_deque.push_back(Box::new(idx+1));
                    big_deque.push_back(Box::new(idx+2));
                    out
                })})
                .collect();

            ths.append(&mut (0..THREAD_COUNT/2).map(|idx| {
                let big_deque = deque.clone();

                std::thread::spawn( move || {
                    big_deque.push_back(Box::new(idx));
                    big_deque.push_back(Box::new(idx+1));
                    let out = big_deque.pop_back().is_none() as i32;
                    big_deque.push_back(Box::new(idx+2));
                    out
                })})
                .collect());

            let _weridness_score = ths.into_iter().map(|th| th.join().unwrap()).sum::<i32>();
            // println!("Weirdness: {weridness_score}");
            // println!("Expected {} found {}", RIGHT_START + THREAD_COUNT*2 + PAD_WIDTH, deque.clone().leftright.load(Ordering::Relaxed) & RIGHTMASK >> 32);
            // despite the fact that each thread should contribute net +1 push into the listing,
            // there's a stochastic event, which may occur, such that all threads behaving this way simultaneously
            // observe and empty stack when popping, and so there's a chance of 'failed' pops.
            assert!(RIGHT_START + THREAD_COUNT*2 + PAD_WIDTH <= ((deque.clone().leftright.load(Ordering::Relaxed) & RIGHTMASK) >> 32) as u32);
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