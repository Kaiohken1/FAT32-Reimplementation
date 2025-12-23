use core::mem::size_of;
use bitmask_enum::bitmask;

//TODO Passer en liste circulaire
struct ListNode {
    size: usize,
    pub next: Option<*mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
struct BufCtl(u32);

#[repr(C)]
struct Slab {
    list: ListNode,
    s_mem: *mut u8,
    inuse: usize,
    free: BufCtl,
}

impl Slab {
    pub fn new(page_start: *mut u8) -> Slab {
        let list = ListNode::new(0);
        let s_mem = page_start;
        let inuse = 0;
        let free = BufCtl(0);
        Slab {
            list,
            s_mem,
            inuse,
            free,
        }
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
    slabs_full: ListNode::new(0),
    slabs_partial: ListNode::new(0),
    slabs_free: ListNode::new(0),
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
        Cache {
            slabs_full: ListNode::new(0),
            slabs_partial: ListNode::new(0),
            slabs_free: ListNode::new(0),
            objsize: size,
            num: 4096 / size,
            gfporder: 0,
            flags,
            spinlock: spin::Mutex::new(()),
            name,
            next: None,
            prev: None,
        }
    }

}

unsafe fn kmem_cache_alloc_one(cache: *mut Cache, flag: Flags) {
    unsafe {
        let slabs_partial = &mut (*cache).slabs_partial;
        let mut entry = slabs_partial.next;

        if entry == Some(slabs_partial) {
            let slabs_free = &mut (*cache).slabs_free;
            entry = slabs_free.next;

            if entry == Some(slabs_free) {
                alloc_new_slab();
            }

            list_del(entry);
            list_add(entry, slabs_partial)
        }

        let slabp = list_entry(entry);
        kmem_cache_alloc_one_tail(cache, slabp);
    }
}

fn alloc_new_slab() {
    todo!()
}

fn list_del(entry: Option<*mut ListNode>) {
    todo!()
}

fn list_add(entry: Option<*mut ListNode>, slabs: &mut ListNode) {
    todo!()
}

fn list_entry(entry: Option<*mut ListNode>) -> *mut Slab {
    todo!()
}

fn kmem_cache_alloc_one_tail(cache: *mut Cache, slab: *mut Slab) {
    todo!();
}

