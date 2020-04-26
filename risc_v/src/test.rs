// test.rs

use crate::{cpu::{build_satp,
                  memcpy,
                  satp_fence_asid,
                  CpuMode,
                  SatpMode,
                  TrapFrame},
            kmem::{kfree, kmalloc},
            page::{map, zalloc, EntryBits, Table, PAGE_SIZE},
            process::{Process,
                      ProcessData,
                      ProcessState,
                      NEXT_PID,
                      PROCESS_LIST,
                      PROCESS_STARTING_ADDR,
                      STACK_ADDR,
                      STACK_PAGES},
            syscall::syscall_fs_read};

pub fn test_block() {
	// The bytes to read would usually come from the inode, but we are in an
	// interrupt context right now, so we cannot pause. Usually, this would be done
	// by an exec system call.
	let bytes_to_read = 1024 * 50;
	let buffer = kmalloc(bytes_to_read);
	// Read the file from the disk.
	let bytes_read = syscall_fs_read(8, 8, buffer, bytes_to_read as u32, 0);
	// After compiling our program, I manually looked and saw it was 12,288
	// bytes. So, to make sure we got the right one, I do a manual check
	// here.
	if bytes_read != 12288 {
		println!(
		         "Unable to load program at inode 8, which should be \
		          12,288 bytes, got {}",
		         bytes_read
		);
	}
	else {
		// Let's get this program running!
		// Everything is "page" based since we're going to map pages to
		// user space. So, we need to know how many program pages we
		// need. Each page is 4096 bytes.
		let program_pages = (bytes_read / PAGE_SIZE) + 1;
		let my_pid = unsafe { NEXT_PID + 1 };
		unsafe {
			NEXT_PID += 1;
		}
		let mut my_proc =
			Process { frame:       zalloc(1) as *mut TrapFrame,
			          stack:       zalloc(STACK_PAGES),
			          pid:         my_pid,
			          root:        zalloc(1) as *mut Table,
			          state:       ProcessState::Running,
			          data:        ProcessData::zero(),
			          sleep_until: 0,
			          program:     zalloc(program_pages), };
		// Map the program in the MMU.
		let ptr = my_proc.program;
		unsafe {
			memcpy(ptr, buffer, bytes_read);
		}
		let table = unsafe { my_proc.root.as_mut().unwrap() };
		// This will map all of the program pages. Notice that in linker.lds in userspace
		// we set the entry point address to 0x2000_0000. This is the same address as
		// PROCESS_STARTING_ADDR, and they must match.
		for i in 0..program_pages {
			let vaddr = PROCESS_STARTING_ADDR + i * PAGE_SIZE;
			let paddr = ptr as usize + i * PAGE_SIZE;
			// We don't have an ELF loader yet, so we're loading raw binaries into memory. Since
			// it is a flat binary, all .data, .rodata, and .bss sections get wrapped into
			// the .text section. Normally, we don't want the .text section to be writeable,
			// however because of this "flattening", we don't have a choice.
			// Notice that USER shows up here. Since we're running in user mode, this bit MUST
			// BE SET! Otherwise, we'll get a page fault from the beginning.
			map(
			    table,
			    vaddr,
			    paddr,
			    EntryBits::UserReadWriteExecute.val(),
			    0,
			);
		}
		// Map the stack
		let ptr = my_proc.stack as *mut u8;
		for i in 0..STACK_PAGES {
			let vaddr = STACK_ADDR + i * PAGE_SIZE;
			let paddr = ptr as usize + i * PAGE_SIZE;
			// We create the stack. We don't load a stack from the disk. This is why I don't
			// need to make the stack executable.
			map(
			    table,
			    vaddr,
			    paddr,
			    EntryBits::UserReadWrite.val(),
			    0,
			);
		}
		// Set everything up in the trap frame
		unsafe {
			// The program counter is a virtual memory address and is loaded into mepc
			// when we execute mret.
			(*my_proc.frame).pc = PROCESS_STARTING_ADDR;
			// Stack pointer. The stack starts at the bottom and works its way up, so we have to
			// set the stack pointer to the bottom.
			(*my_proc.frame).regs[2] =
				STACK_ADDR as usize + STACK_PAGES * PAGE_SIZE;
			// USER MODE! This is how we set what'll go into mstatus when we run the process.
			(*my_proc.frame).mode = CpuMode::User as usize;
			(*my_proc.frame).pid = my_proc.pid as usize;
			// The SATP register is used for the MMU, so we need to
			// map our table into that register. The switch_to_user
			// function will load .satp into the actual register
			// when the time comes.
			(*my_proc.frame).satp =
				build_satp(
				           SatpMode::Sv39,
				           my_proc.pid as usize,
				           my_proc.root as usize,
				);
		}
		// We don't reuse PIDs, so this really shouldn't matter.
		satp_fence_asid(my_pid as usize);
		// I took a different tact here than in process.rs. In there I created the process
		// while holding onto the process list. It doesn't really matter since this is synchronous,
		// but it might get dicey 
		if let Some(mut pl) = unsafe { PROCESS_LIST.take() } {
			println!(
			         "Added user process to the scheduler...get \
			          ready for take-off!"
			);
			// As soon as we push this process on the list, it'll be schedule-able.
			pl.push_back(my_proc);
			unsafe {
				PROCESS_LIST.replace(pl);
			}
		}
		else {
			println!("Unable to spawn process.");
			// Since my_proc couldn't enter the process list, it
			// will be dropped and all of the associated allocations
			// will be deallocated.
		}
	}
	println!();
	kfree(buffer);
}
