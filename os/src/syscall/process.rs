//! Process management syscalls
use crate::mm::FRAME_ALLOCATOR;
use crate::mm::{translated_byte_buffer, VirtAddr, VirtPageNum};
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_user_token, get_page_table_entry, TASK_MANAGER};
use crate::timer::get_time_us;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
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
    trace!("kernel: sys_get_time");

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

/// TODO: Finish sys_trace to pass testcases
///
/// - trace_request == 0: Read a u8 from user address `id` (as *const u8), ignore `data`, return the value.
/// - trace_request == 1: Write `data` (as u8) to user address `id` (as *mut u8), return 0 on success.
/// - trace_request == 2: Query the syscall count for syscall number `id` for current task, return the count (this call also counts).
/// - Otherwise: return -1.
///
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    // 取得当前用户的token
    let user_token = current_user_token();

    match trace_request {
        0 => {
            // 1. 判断id是否是有效的, 属于当前用户空间的虚拟地址, 且有读权限, 否则返回-1
            // 2. 读取并返回该地址的值
            get_page_table_entry(VirtAddr::from(id))
                .map(|page_table_entry| {
                    if page_table_entry.is_user() && page_table_entry.readable() {
                        let user_buffer_start = id as *const u8;
                        // 取得内核空间的buffer
                        let kernel_buffer =
                            translated_byte_buffer(user_token, user_buffer_start, size_of::<u8>());
                        // 由于这里只需要读取1个u8, 所以只需要取第一段的第一个元素
                        kernel_buffer[0][0] as isize
                    } else {
                        -1
                    }
                })
                .unwrap_or(-1) // virtual address not visible for current user
        }
        1 => {
            // 1. 判断id是否是有效的当前用户空间的虚拟地址, 且有写权限, 否则返回-1
            // 2. 将data转化为u8, 写入该地址
            // 3. 返回0
            get_page_table_entry(VirtAddr::from(id))
                .map(|page_table_entry| {
                    if page_table_entry.is_user() && page_table_entry.writable() {
                        let user_buffer_start = id as *const u8;
                        // 取得内核空间的buffer
                        let kernel_buffer =
                            translated_byte_buffer(user_token, user_buffer_start, size_of::<u8>());
                        // 将数据写入目标地址
                        // 由于这里只需要写入1个u8, 所以只需要取第一段的第一个元素
                        if let Some(segment) = kernel_buffer.into_iter().next() {
                            segment[0] = data as u8;
                            0
                        } else {
                            -1
                        }
                    } else {
                        -1
                    }
                })
                .unwrap_or(-1) // virtual address not visible for current user
        }
        2 => {
            // syscall count for current task
            let count = TASK_MANAGER.get_syscall_count(id);
            count as isize
        }
        _ => -1,
    }
}

/// TODO: Implement mmap.
///
/// - 申请长度为 len 字节的物理内存(不要求实际物理内存位置, 可以随便找一块),
/// 将其映射到 start 开始的虚存，内存页属性为 prot
///
/// - [start, start + len), 左闭右开
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap IMPLEMENTED !");

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

    // 分配页帧
    TASK_MANAGER.mmap(start_virtual_addr, end_virtual_addr, prot);
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

    TASK_MANAGER.munmap(start_virtual_page_number, end_virtual_page_number)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
