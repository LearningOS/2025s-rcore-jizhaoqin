//! Process management syscalls

use crate::loader::get_app_data_by_name;
use crate::mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission};
use crate::mm::{VirtAddr, VirtPageNum, FRAME_ALLOCATOR};
use crate::task::{add_task, current_task, current_user_token, get_page_table_entry};
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next};
use crate::timer::get_time_us;
use alloc::sync::Arc;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
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
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
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
///
/// - `ts`为用户空间`TimeVal`的虚拟地址, 而此函数运行在内核态, 直接使用`ts`会被视为内核空间的虚拟地址,
/// 从而造成错误的地址访问. 需要将应用地址空间中的对应缓冲区转化为在内核空间中能够直接访问的形式
///
/// - translated_byte_buffer能正确处理跨页
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    // 取得当前用户的token
    let user_token = current_user_token();
    // 取得当前用户目标buffer的起始地址(用户空间虚拟地址)
    let user_buffer_start = ts as *const u8;
    // 查询页表, 转换为内核空间的buffer可访问的形式
    // 这个buffer被拆分为数段(因为`TimeVal`的储存可能跨页)连续的[u8]并保存其指针
    // 而非为每一个的u8字段保存指针
    let kernel_buffer = translated_byte_buffer(user_token, user_buffer_start, size_of::<TimeVal>());

    // 取得当前时间并转化为`TimeVal`结构体
    let micro_seconds = get_time_us();
    let time = TimeVal {
        sec: micro_seconds / 1_000_000,
        usec: micro_seconds % 1_000_000,
    };
    // 将time转化为连续的u8字节数组
    let time_bytes = unsafe {
        core::slice::from_raw_parts(&time as *const TimeVal as *const u8, size_of::<TimeVal>())
    };

    // 将时间写入内核空间的buffer
    let mut offset = 0;
    for segment in kernel_buffer {
        let segment_len = segment.len();
        // 为每段[u8]写入对应位置和长度的time_bytes数据
        segment.copy_from_slice(&time_bytes[offset..offset + segment_len]);
        offset += segment_len;
    }

    0
}

/// TODO: Implement mmap.
///
/// - 申请长度为 len 字节的物理内存(不要求实际物理内存位置, 可以随便找一块),
/// 将其映射到 start 开始的虚存，内存页属性为 prot
///
/// - [start, start + len), 左闭右开
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    // check port
    if prot & !7 != 0 || prot & 7 == 0 {
        return -1;
    }

    let start_virtual_addr = VirtAddr::from(start);
    // check alignment
    if !start_virtual_addr.aligned() {
        return -1;
    }
    let start_virtual_page_number = start_virtual_addr.floor();

    let end_virtual_addr = VirtAddr::from(start + len); // len*8如果len是字节长度的话 ?
    let end_virtual_page_number = end_virtual_addr.ceil();

    // 检查[start, start + len) 中如果存在已经被映射的页则返回-1
    for virtual_page_number in start_virtual_page_number.0..end_virtual_page_number.0 {
        // 根据虚拟页号取得对齐的虚拟地址
        let virtual_address = VirtAddr::from(VirtPageNum(virtual_page_number));
        let page_table_entry = get_page_table_entry(virtual_address);
        if let Some(entry) = page_table_entry {
            if entry.is_valid() {
                // 该页已经被映射, 返回-1
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
///
/// - 取消到 [start, start + len) 虚存的映射。特别地，在 rCore 课程实验中,
/// 正确执行的 sys_munmap 仅会对应 唯一且完整 的 mmap 区间，不考虑交叉、截断区间的情况
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

    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(data);
        let new_pid = new_task.pid.0;
        // add new task to scheduler
        add_task(new_task);
        return new_pid as isize;
    }

    -1
}

/// TODO: Set task priority.
///
/// - syscall ID：140
/// - 设置当前进程优先级为 priority
/// - 参数：priority 进程优先级，要求 priority >= 2
/// - 返回值：如果输入合法则返回 priority，否则返回 -1
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
