struct ListNode {
    size: usize,
    next: Option<*mut ListNode>,
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
        Slab { list, s_mem, inuse, free }
    }
}

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
    name : &'static str,
    gfporder: usize,

    next: Option<*mut Cache>,
    prev: Option<*mut Cache>,
}

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