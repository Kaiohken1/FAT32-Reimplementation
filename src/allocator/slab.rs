use crate::allocator::Locked;
use alloc::alloc::Layout;
use core::{
    alloc::GlobalAlloc,
    mem::{align_of, size_of},
    ptr::{null_mut, write},
};
use x86_64::VirtAddr;

const PAGE_SIZE: usize = 4096;
const MAX_SLAB_SIZE: usize = 2048;
const MAX_CLASSES: usize = 9;

#[repr(C)]
struct ListNode {
    next: *mut ListNode,
    prev: *mut ListNode,
}

impl ListNode {
    const fn new() -> Self {
        Self {
            next: null_mut(),
            prev: null_mut(),
        }
    }
    unsafe fn init(node: *mut ListNode) {
        unsafe {
            (*node).next = node;
            (*node).prev = node;
        }
    }
}

unsafe fn list_add(entry: *mut ListNode, head: *mut ListNode) {
    unsafe {
        (*entry).next = (*head).next;
        (*entry).prev = head;
        (*(*head).next).prev = entry;
        (*head).next = entry;
    }
}

unsafe fn list_del(entry: *mut ListNode) {
    unsafe {
        (*(*entry).prev).next = (*entry).next;
        (*(*entry).next).prev = (*entry).prev;
    }
}

unsafe fn list_empty(head: *mut ListNode) -> bool {
    unsafe { (*head).next == head }
}

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq)]
struct BufCtl(u32);
impl BufCtl {
    const END: Self = BufCtl(u32::MAX);
}

#[repr(C)]
struct Slab {
    list: ListNode,
    s_mem: *mut u8,
    free: BufCtl,
    inuse: usize,
}

#[repr(C)]
struct Cache {
    slabs_full: ListNode,
    slabs_partial: ListNode,
    slabs_free: ListNode,
    obj_size: usize,
    num: usize,
    name: &'static str,
}

pub struct SlabAllocator {
    page_alloc: Option<PageAllocator>,
    node_caches: [*mut Cache; MAX_CLASSES],
}

struct PageAllocator {
    next: VirtAddr,
    end: VirtAddr,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        Self {
            page_alloc: None,
            node_caches: [null_mut(); MAX_CLASSES],
        }
    }

    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.page_alloc = Some(PageAllocator {
            next: VirtAddr::new(heap_start as u64),
            end: VirtAddr::new((heap_start + heap_size) as u64),
        });
    }

    unsafe fn get_or_create_cache(&mut self, size: usize) -> *mut Cache {
        let idx = (size.trailing_zeros() as usize).saturating_sub(3);
        if self.node_caches[idx].is_null() {
            let page = self
                .page_alloc
                .as_mut()
                .unwrap()
                .alloc_pages(1)
                .expect("OOM Cache");
            let cache_ptr = page.as_u64() as *mut Cache;

            let obj_size = size.max(8).next_power_of_two();
            let num = (PAGE_SIZE - size_of::<Slab>()) / (obj_size + size_of::<BufCtl>());

            unsafe {
                write(
                    cache_ptr,
                    Cache {
                        slabs_full: ListNode::new(),
                        slabs_partial: ListNode::new(),
                        slabs_free: ListNode::new(),
                        obj_size,
                        num,
                        name: "generic_cache",
                    },
                );

                ListNode::init(&mut (*cache_ptr).slabs_full);
                ListNode::init(&mut (*cache_ptr).slabs_partial);
                ListNode::init(&mut (*cache_ptr).slabs_free);
            }
            self.node_caches[idx] = cache_ptr;
        }
        self.node_caches[idx]
    }
}

unsafe fn cache_grow(page_alloc: &mut PageAllocator, cache: *mut Cache) {
    let page = page_alloc.alloc_pages(1).expect("OOM Slab");
    let slab_ptr = page.as_u64() as *mut Slab;

    let bufctl_ptr = (page.as_u64() as usize + size_of::<Slab>()) as *mut BufCtl;

    unsafe {
        let bufctl_table_size = (*cache).num * size_of::<BufCtl>();
        let obj_start = (bufctl_ptr as usize + bufctl_table_size + align_of::<usize>() - 1)
            & !(align_of::<usize>() - 1);

        write(
            slab_ptr,
            Slab {
                list: ListNode::new(),
                s_mem: obj_start as *mut u8,
                free: BufCtl(0),
                inuse: 0,
            },
        );

        for i in 0..(*cache).num {
            let next = if i + 1 == (*cache).num {
                BufCtl::END
            } else {
                BufCtl((i + 1) as u32)
            };
            bufctl_ptr.add(i).write(next);
        }

        list_add(&mut (*slab_ptr).list, &mut (*cache).slabs_free);
    }
}

unsafe impl GlobalAlloc for Locked<SlabAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        let size = layout.size();

        if size > MAX_SLAB_SIZE {
            let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
            return allocator
                .page_alloc
                .as_mut()
                .unwrap()
                .alloc_pages(num_pages)
                .map_or(null_mut(), |p| p.as_u64() as *mut u8);
        }

        unsafe {
            let cache = allocator.get_or_create_cache(size);

            let slab_node = if !list_empty(&mut (*cache).slabs_partial) {
                (*cache).slabs_partial.next
            } else {
                if list_empty(&mut (*cache).slabs_free) {
                    cache_grow(allocator.page_alloc.as_mut().unwrap(), cache);
                }
                let node = (*cache).slabs_free.next;
                list_del(node);
                list_add(node, &mut (*cache).slabs_partial);
                node
            };

            let slab = slab_node as *mut Slab;
            let obj_idx = (*slab).free.0 as usize;
            let obj = (*slab).s_mem.add(obj_idx * (*cache).obj_size);

            let bufctl_ptr = (slab as usize + size_of::<Slab>()) as *mut BufCtl;
            (*slab).free = *bufctl_ptr.add(obj_idx);
            (*slab).inuse += 1;

            if (*slab).inuse == (*cache).num {
                list_del(&mut (*slab).list);
                list_add(&mut (*slab).list, &mut (*cache).slabs_full);
            }

            obj
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        let size = layout.size();
        if size > MAX_SLAB_SIZE || ptr.is_null() {
            return;
        }
        unsafe {
            let cache = allocator.get_or_create_cache(size);

            let slab = (ptr as usize & !(PAGE_SIZE - 1)) as *mut Slab;
            let obj_idx = (ptr as usize - (*slab).s_mem as usize) / (*cache).obj_size;

            let bufctl_ptr = (slab as usize + size_of::<Slab>()) as *mut BufCtl;

            bufctl_ptr.add(obj_idx).write((*slab).free);
            (*slab).free = BufCtl(obj_idx as u32);
            (*slab).inuse -= 1;

            if (*slab).inuse + 1 == (*cache).num {
                list_del(&mut (*slab).list);
                list_add(&mut (*slab).list, &mut (*cache).slabs_partial);
            } else if (*slab).inuse == 0 {
                list_del(&mut (*slab).list);
                list_add(&mut (*slab).list, &mut (*cache).slabs_free);
            }
        }
    }
}

impl PageAllocator {
    fn alloc_pages(&mut self, num: usize) -> Option<VirtAddr> {
        let bytes = (num * PAGE_SIZE) as u64;
        if self.next + bytes > self.end {
            return None;
        }
        let addr = self.next;
        self.next += bytes;
        Some(addr)
    }
}

unsafe impl Send for SlabAllocator {}
unsafe impl Sync for SlabAllocator {}
