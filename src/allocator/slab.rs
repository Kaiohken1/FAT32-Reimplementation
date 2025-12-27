use bitmask_enum::bitmask;
use core::{mem::size_of, ptr::null_mut, u32};

struct ListNode {
    pub next: *mut ListNode,
}

impl ListNode {
    pub const fn new() -> Self {
        ListNode { next: null_mut() }
    }

    unsafe fn init(&mut self) {
        let self_ptr = self as *mut ListNode;
        self.next = self_ptr;
    }
}

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq)]
struct BufCtl(u32);

impl BufCtl {
    pub const BUFCTL_END: BufCtl = BufCtl(u32::MAX);
}

#[repr(C)]
struct Slab {
    list: ListNode,
    s_mem: *mut u8,
    inuse: usize,
    free: BufCtl,
}

impl Slab {
    pub fn new(page_start: *mut u8) -> Slab {
        let mut slab = Slab {
            list: ListNode::new(),
            s_mem: page_start,
            inuse: 0,
            free: BufCtl(0),
        };

        unsafe {
            slab.list.init();
        }

        slab
    }
}

#[bitmask]
enum Flags {
    CfgsOffSlab,
    CflgsOptimize,
    SlabHwcacheAlign,
    SlabMustHwcacheAlign,
    SlabNoReap,
    SlabCacheDma,
    SlabDebugFree,
    SlabDebugInitial,
    SlabRedZone,
    SlabPoison,
}

#[repr(C)]
struct Cache {
    slabs_full: ListNode,
    slabs_partial: ListNode,
    slabs_free: ListNode,
    objsize: usize,
    flags: Flags,
    num: usize,
    spinlock: spin::Mutex<()>,
    name: &'static str,
    gfporder: usize,

    next: Option<*mut Cache>,
    prev: Option<*mut Cache>,
}

static mut KMEM_CACHE: Cache = Cache {
    slabs_full: ListNode::new(),
    slabs_partial: ListNode::new(),
    slabs_free: ListNode::new(),
    objsize: size_of::<Cache>(),
    num: 4096 / size_of::<Cache>(),
    gfporder: 0,
    flags: Flags::CfgsOffSlab,
    spinlock: spin::Mutex::new(()),
    name: "kmem_cache",
    next: None,
    prev: None,
};

impl Cache {
    pub fn new(name: &'static str, size: usize, flags: Flags) -> Cache {
        let mut cache = Cache {
            slabs_full: ListNode::new(),
            slabs_partial: ListNode::new(),
            slabs_free: ListNode::new(),
            objsize: size,
            num: 4096 / size,
            gfporder: 0,
            flags,
            spinlock: spin::Mutex::new(()),
            name,
            next: None,
            prev: None,
        };

        unsafe {
            cache.slabs_full.init();
            cache.slabs_partial.init();
            cache.slabs_free.init();

            cache
        }
    }
}

unsafe fn kmem_cache_alloc_one(cache: *mut Cache, flag: Flags) -> *mut u8 {
    unsafe {
        let slabs_partial = &mut (*cache).slabs_partial;
        let mut entry = slabs_partial.next;

        if entry == slabs_partial {
            let slabs_free = &mut (*cache).slabs_free;
            entry = slabs_free.next;

            if entry == slabs_free {
                alloc_new_slab();
                entry = slabs_free.next;
            }

            list_del(entry);
            list_add(entry, slabs_partial)
        }

        let slabp: *mut Slab = list_entry(entry);
        kmem_cache_alloc_one_tail(cache, slabp)
    }
}

unsafe fn kmem_cache_alloc_one_tail(cache: *mut Cache, slab: *mut Slab) -> *mut u8 {
    unsafe {
        let s = &mut *slab;

        let obj_index = s.free;
        s.inuse += 1;

        let obj = s.s_mem.add(obj_index.0 as usize * (*cache).objsize);

        let bufctl_array = (slab.add(1)) as *mut BufCtl;

        let next_free = *bufctl_array.add(obj_index.0 as usize);
        s.free = next_free;

        if s.free == BufCtl::BUFCTL_END {
            list_del(&mut s.list);
            list_add(&mut s.list, &mut (*cache).slabs_full);
        }

        obj
    }
}

fn alloc_new_slab() {
    todo!()
}

fn list_del(entry: *mut ListNode) {
    todo!()
}

fn list_add(entry: *mut ListNode, slabs: &mut ListNode) {
    todo!()
}

fn list_entry(entry: *mut ListNode) -> *mut Slab {
    todo!()
}
