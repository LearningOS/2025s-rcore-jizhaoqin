//! Process management syscalls
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next, TASK_MANAGER};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

/// syscall for tracing and manipulating user memory or syscall statistics
///
/// - trace_request == 0: Read a u8 from user address `id` (as *const u8), ignore `data`, return the value.
/// - trace_request == 1: Write `data` (as u8) to user address `id` (as *mut u8), return 0 on success.
/// - trace_request == 2: Query the syscall count for syscall number `id` for current task, return the count (this call also counts).
/// - Otherwise: return -1.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        0 => {
            let addr = id as *const u8;
            let value = unsafe { core::ptr::read_volatile(addr) };
            value as isize
        }
        1 => {
            let addr = id as *mut u8;
            unsafe { core::ptr::write_volatile(addr, data as u8) };
            0
        }
        2 => {
            // syscall count for current task
            let count = TASK_MANAGER.get_syscall_count(id);
            count as isize
        }
        _ => -1,
    }
}
