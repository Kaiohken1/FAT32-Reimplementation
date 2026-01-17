//! Implémentation d’un allocateur de type SLAB classique
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

/// Structure de liste circulaire doublement chaînée
#[repr(C)]
struct ListNode {
    /// Pointeur vers l'élément précédent de la liste
    next: *mut ListNode,

    /// Pointeur vers l'élément suivant de la liste
    prev: *mut ListNode,
}

impl ListNode {
    /// Création de la liste avec des pointeur nuls
    const fn new() -> Self {
        Self {
            next: null_mut(),
            prev: null_mut(),
        }
    }
/// Initialise un noeud comme liste chaînée circulaire vide.
/// 
/// # Safety
/// `node` doit être un pointeur valide et correctement aligné vers un `ListNode` initialisé.
    unsafe fn init(node: *mut ListNode) {
        unsafe {
            (*node).next = node;
            (*node).prev = node;
        }
    }
}

/// Ajout d'un élement depuis la queue de la liste
/// 
/// # Safety
/// `entry` et `head` doivent être des pointeurs valides et correctement aligné vers un `ListNode` initialisé.
unsafe fn list_add(entry: *mut ListNode, head: *mut ListNode) {
    unsafe {
        (*entry).next = (*head).next;
        (*entry).prev = head;
        (*(*head).next).prev = entry;
        (*head).next = entry;
    }
}

/// Suppression d'un élement de la liste
/// # Safety
/// `entry` doit être un pointeur valide et correctement aligné vers un `ListNode` initialisé.
unsafe fn list_del(entry: *mut ListNode) {
    unsafe {
        (*(*entry).prev).next = (*entry).next;
        (*(*entry).next).prev = (*entry).prev;
    }
}

/// Vérifie si une liste est vide ou non
/// 
/// # Safety
/// `head` doit être un pointeur valide et correctement aligné vers un `ListNode` initialisé.
unsafe fn list_empty(head: *mut ListNode) -> bool {
    unsafe { (*head).next == head }
}

/// Tableau d'index du prochain objet libre pour utilisation
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq)]
struct BufCtl(u32);
impl BufCtl {
    const END: Self = BufCtl(u32::MAX);
}

/// Représentation du Slab
#[repr(C)]
struct Slab {
    /// Liste chaînée du cache dans lequel appartient le slab
    list: ListNode,
    
    /// Addresse de départ du premier objet du slab
    s_mem: *mut u8,
    
    /// Index du prochain objet libre pour utilisation
    free: BufCtl,
    
    /// Nombre d'objets utilisés
    inuse: usize,
}

/// Rprésentation du Cache
#[repr(C)]
struct Cache {
    /// Liste chaînée de slabs pleins
    slabs_full: ListNode,
    
    /// Liste chaînée de slabs partiellement remplis
    slabs_partial: ListNode,
    
    /// Liste chaînée de slabs vide
    slabs_free: ListNode,
    
    /// Taille de chaque objet contenus dans un slab
    obj_size: usize,

    /// Nombre d'objets contenus dans un slab
    num: usize,

    /// Nom du cache
    name: &'static str,
}

/// Représentation de l'allocateur Slab
pub struct SlabAllocator {
    /// Allocateur de page
    page_alloc: Option<PageAllocator>,

    /// Tableau de caches générés par l'allocateur
    node_caches: [*mut Cache; MAX_CLASSES],
}

/// Représentation de l'allocateur de pages
struct PageAllocator {
    next: VirtAddr,
    end: VirtAddr,
}

/// Liste des noms de caches selon la taille allouée
const CACHE_NAMES: [&'static str; MAX_CLASSES] = [
    "size-8",
    "size-16",
    "size-32",
    "size-64",
    "size-128",
    "size-256",
    "size-512",
    "size-1024",
    "size-2048",
];

impl SlabAllocator {
    /// Création basique d'un alloacteur avec toutes ses paramètres non initialisés
    pub const fn new() -> Self {
        Self {
            page_alloc: None,
            node_caches: [null_mut(); MAX_CLASSES],
        }
    }

    /// Initialisation de l'allocateur de page de l'allocateur de slab. 
    /// Cette fonction ne doit être appellée qu'une seule fois par démarrage
    /// 
    /// # Safety
    /// - `heap_start` doit être aligné sur la taille de page.
    /// - `heap_size` doit être un multiple de la taille de page.
    /// - La plage de la heap doit être valide, correctement initialisée, et exclusivement utilisée par cet allocateur.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.page_alloc = Some(PageAllocator {
            next: VirtAddr::new(heap_start as u64),
            end: VirtAddr::new((heap_start + heap_size) as u64),
        });
    }

    /// Recherche ou crée un cache pour une taille d’objet donnée.
    ///
    /// # Safety
    /// - `self.page_alloc` doit être initialisé.
    /// - La plage mémoire retournée par `alloc_pages(1)` doit être valide et correctement alignée pour `Cache`.
    /// - `self.node_caches` doit avoir une taille suffisante pour l’index calculé à partir de `size`.
    /// - `size` doit être > 0.
    unsafe fn get_or_create_cache(&mut self, size: usize) -> *mut Cache {
        let idx = (size.trailing_zeros() as usize).saturating_sub(3);

        assert!(size > 0);

        let name = if idx < CACHE_NAMES.len() {
            CACHE_NAMES[idx]
        } else {
            "size-large"
        };

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
                        name,
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

/// Ajout d'un slab dans un cache
/// Cette fonction alloue une page et y écrit le nouveau slab et son tableau bufctl
/// 
/// # Safety
/// - `self.page_alloc` doit être initialisé.
/// - La page retournée doit être correctement alignée pour `Slab` et `BufCtl`.
/// - `cache` doit être un pointeur valide vers un `Cache` initialisé.
/// - `cache.num` et `cache.obj_size` doivent être cohérents avec la taille de page.
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
    /// Allocation d'un bloc de mémoire
    ///
    /// # Safety
    /// - L'allocateur doit être correctement initialisé avant tout appel.
    /// - Les appels concurrents doivent être protégés par le verrou `Locked`.
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

    /// Libère un bloc précédemment alloué par cet allocateur.
    ///
    /// # Safety
    /// - `ptr` doit provenir d’un appel à `alloc` de cet allocateur.
    /// - `layout` doit correspondre exactement à celui utilisé lors de l’allocation.
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
