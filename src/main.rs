#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(fat32_impl::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::rc::Rc;
// use alloc::{boxed::Box, vec, vec::Vec, rc::Rc};
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use fat32_impl::file_system::Fat32FileSystem;
use fat32_impl::file_system::ShellSession;
use fat32_impl::println;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use fat32_impl::allocator;
    use fat32_impl::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    println!("Hello World{}", "!");
    fat32_impl::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    const DISK_IMAGE: &[u8] = include_bytes!("../test.img");

    let fs = Fat32FileSystem::new(DISK_IMAGE);

    let mut shell_session = ShellSession::new(Rc::new(fs));

    shell_session.ls();

    shell_session.cd("test_dir").unwrap();

    shell_session.ls();

    shell_session.cd("../").unwrap();

    shell_session.ls();

    #[cfg(test)]
    test_main();

    println!("Did not crash!");
    fat32_impl::hlt_loop();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    fat32_impl::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    fat32_impl::test_panic_handler(info)
}
