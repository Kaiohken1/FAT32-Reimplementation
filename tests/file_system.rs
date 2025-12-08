#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(fat32_impl::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::string::String;
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use fat32_impl::disk::Fat32FileSystem;
use fat32_impl::disk::{list_directory_entries, list_files_names};
use fat32_impl::serial_print;

entry_point!(main);

const DISK_IMAGE: &[u8] = include_bytes!("./test.img");

#[test_case]
fn read_test() {
    let fs = Fat32FileSystem::new(DISK_IMAGE);

    let files = list_directory_entries(&fs, fs.root_cluster);

    for file in files {
        let mut data = fs.read_file(file.start_cluster);
        data.truncate(file.size as usize);
        let data_string = String::from_utf8(data).unwrap();
        assert_eq!("test\n", data_string);
    }
}

#[test_case]
fn ls_test() {
    let fs = Fat32FileSystem::new(DISK_IMAGE);

    let files = list_directory_entries(&fs, fs.root_cluster);
    let files_list = list_files_names(&files);

    assert_eq!(["test.txt"], files_list.as_slice());
}

#[test_case]
fn init_test() {
    let fs = Fat32FileSystem::new(DISK_IMAGE);
    let root_data = fs.read_cluster(fs.root_cluster);
    assert_ne!(0, fs.data_sector);
    assert_ne!(0, fs.fat_sector);
    assert!(fs.root_cluster >= 2);
    assert_ne!(0, root_data.len());

    let expected_size = (fs.sectors_per_cluster * fs.bytes_per_sector) as usize;
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
