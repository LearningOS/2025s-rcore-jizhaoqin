//! Process management syscalls
//!
use crate::fs::{open_file, OpenFlags};
use crate::mm::{
    translated_byte_buffer, translated_refmut, translated_str, MapPermission, VirtAddr,
    VirtPageNum, FRAME_ALLOCATOR,
};
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next, get_page_table_entry,
    suspend_current_and_run_next,
};
use crate::timer::get_time_us;
use alloc::sync::Arc;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// TODO: get time with second and microsecond
///
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let user_token = current_user_token();
    let user_buffer_start = ts as *const u8;
    let kernel_buffer = translated_byte_buffer(user_token, user_buffer_start, size_of::<TimeVal>());

    let micro_seconds = get_time_us();
    let time = TimeVal {
        sec: micro_seconds / 1_000_000,
        usec: micro_seconds % 1_000_000,
    };
    let time_bytes = unsafe {
        core::slice::from_raw_parts(&time as *const TimeVal as *const u8, size_of::<TimeVal>())
    };

    // 将时间写入内核空间的buffer
    let mut offset = 0;
    for segment in kernel_buffer {
        let segment_len = segment.len();
        segment.copy_from_slice(&time_bytes[offset..offset + segment_len]);
        offset += segment_len;
    }

    0
}

/// TODO: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    if prot & !7 != 0 || prot & 7 == 0 {
        return -1;
    }

    let start_virtual_addr = VirtAddr::from(start);
    // check alignment
    if !start_virtual_addr.aligned() {
        return -1;
    }
    let start_virtual_page_number = start_virtual_addr.floor();

    let end_virtual_addr = VirtAddr::from(start + len);
    let end_virtual_page_number = end_virtual_addr.ceil();

    // 检查[start, start + len) 中如果存在已经被映射的页则返回-1
    for virtual_page_number in start_virtual_page_number.0..end_virtual_page_number.0 {
        let virtual_address = VirtAddr::from(VirtPageNum(virtual_page_number));
        let page_table_entry = get_page_table_entry(virtual_address);
        if let Some(entry) = page_table_entry {
            if entry.is_valid() {
                return -1;
            }
        }
    }

    // 检查物理内存是否足够
    if FRAME_ALLOCATOR.exclusive_access().available_page_frames()
        < (end_virtual_page_number.0 - start_virtual_page_number.0)
    {
        return -1;
    }

    let current_task = current_task().unwrap();
    let mut task_inner = current_task.inner_exclusive_access();

    // 处理权限
    let mut map_permission = MapPermission::U;
    if prot & 1 == 1 {
        map_permission |= MapPermission::R;
    }
    if prot & 2 == 2 {
        map_permission |= MapPermission::W;
    }
    if prot & 4 == 4 {
        map_permission |= MapPermission::X;
    }

    // 分配页帧
    task_inner
        .memory_set
        .insert_framed_area(start_virtual_addr, end_virtual_addr, map_permission);
    0
}

/// TODO: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");

    let start_virtual_addr = VirtAddr::from(start);
    let start_virtual_page_number = start_virtual_addr.floor();
    let end_virtual_addr = VirtAddr::from(start + len);
    let end_virtual_page_number = end_virtual_addr.ceil();

    // 检查是否为唯一且完整 的 mmap 区间, 若否则返回-1
    if !start_virtual_addr.aligned() || !end_virtual_addr.aligned() {
        return -1;
    }

    let current_task = current_task().unwrap();
    let mut task_inner = current_task.inner_exclusive_access();
    task_inner
        .memory_set
        .munmap(start_virtual_page_number, end_virtual_page_number)
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// TODO: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let token = current_user_token();
    let path = translated_str(token, path);

    // 从文件系统中读取文件, 再加载到内存, 而非一开就始将所有文件加载到内存中
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(all_data.as_slice());
        let new_pid = new_task.pid.0;
        // add new task to scheduler
        add_task(new_task);
        return new_pid as isize;
    }
    -1
}

/// TODO: Set task priority.
pub fn sys_set_priority(priority: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    // 检查priority是否合法
    if priority <= 2 {
        return -1;
    }

    let current_task = current_task().unwrap();
    current_task.inner_exclusive_access().priority = priority as usize;

    priority
}
