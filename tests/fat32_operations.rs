#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(fat32_impl::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::ToString;
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use fat32_impl::file_system::{Fat32FileSystem, interface::ShellSession};
use fat32_impl::file_system::{list_directory_entries, list_files_names};
use spin::Mutex;

entry_point!(main);

//Image de test contenant initialement un fichier et un dossier de test
// [/] > file.txt et test_dir
// [test_dir] > test_dir_file
const DISK_IMAGE: &[u8] = include_bytes!("./test.img");

//TODO Trouver une méthode plus optimisée pour charger le file system une seule fois
fn init_fs() -> Rc<Mutex<Fat32FileSystem>> {
    let disk_box = alloc::vec::Vec::from(DISK_IMAGE).into_boxed_slice();
    let fs = Fat32FileSystem::new(disk_box);

    Rc::new(Mutex::new(fs))
}

#[test_case]
fn write_test() {
    let fs = init_fs();
    let shell = ShellSession::new(fs.clone());

    shell.touch("", "FILE_T").expect("Erreur lors du touch");

    shell.write("FILE_T", "write test").expect("erreur lors du write");

    let data = match fs.lock().read_file("/FILE_T", None) {
        Ok(content) => content,
        Err(e) => e.to_string(),
    };
    assert_eq!("write test", data);
}

#[test_case]
fn touch_test() {
    let fs = init_fs();
    let mut shell = ShellSession::new(fs.clone());

    shell.touch("", "FILE_T").expect("Erreur lors du touch");

    let entries = shell.ls_entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2].name, "FILE_T");
    assert!(!entries[2].is_directory);

    shell.cd("test_dir").unwrap();
    shell
        .touch("test_dir", "FILE_T2")
        .expect("Erreur lors du touch");

    let entries = shell.ls_entries();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].name, "FILE_T2");
    assert!(!entries[1].is_directory);
}

#[test_case]
fn mkdir_test() {
    let fs = init_fs();
    let mut shell = ShellSession::new(fs.clone());

    shell.mkdir("", "DIR_T").expect("Erreur lors du mkdir");

    let entries = shell.ls_entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2].name, "DIR_T");
    assert!(entries[2].is_directory);

    shell.cd("test_dir").unwrap();
    shell
        .mkdir("test_dir", "DIR_T2")
        .expect("Erreur lors du mkdir");

    let entries = shell.ls_entries();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].name, "DIR_T2");
    assert!(entries[1].is_directory);
}

#[test_case]
fn cd_test() {
    let fs = init_fs();

    let mut shell = ShellSession::new(fs.clone());

    let root_ls = shell.ls_entries();
    assert!(root_ls.iter().any(|e| e.name == "test_dir"));

    shell.cd("test_dir").unwrap();
    let test_dir_ls = shell.ls_entries();
    assert!(test_dir_ls.iter().any(|e| e.name == "test_dir_file"));

    shell.cd("..").unwrap();
    let back_ls = shell.ls_entries();
    assert!(back_ls == root_ls);
}

#[test_case]
fn read_test() {
    let fs = init_fs();
    let fs_lock = fs.lock();

    let data = match fs_lock.read_file("/test_dir/test_dir_file", None) {
        Ok(content) => content,
        Err(e) => e.to_string(),
    };
    assert_eq!("test d'écriture dans un fichier d'un dossier\n", data);
}

#[test_case]
fn ls_test() {
    let fs = init_fs();
    let fs_lock = fs.lock();

    let files = list_directory_entries(&fs_lock, fs_lock.root_cluster);
    let files_list = list_files_names(&files);

    assert_eq!(["test.txt", "test_dir"], files_list.as_slice());
}

#[test_case]
fn init_test() {
    let fs = init_fs();
    let fs_lock = fs.lock();

    let root_data = fs_lock.read_cluster(fs_lock.root_cluster);
    assert_ne!(0, fs_lock.data_sector);
    assert_ne!(0, fs_lock.fat_sector);
    assert!(fs_lock.root_cluster >= 2);
    assert_ne!(0, root_data.len());

    let expected_size = (fs_lock.sectors_per_cluster * fs_lock.bytes_per_sector) as usize;
    assert_eq!(
        expected_size,
        root_data.len(),
        "La taille de la racine lue est incorrecte."
    );
}

fn main(boot_info: &'static BootInfo) -> ! {
    use fat32_impl::allocator;
    use fat32_impl::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    fat32_impl::init();
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    fat32_impl::test_panic_handler(info)
}
