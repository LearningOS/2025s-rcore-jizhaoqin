//! File and filesystem-related syscalls
use core::mem::size_of;

use crate::fs::{open_file, OpenFlags, Stat, ROOT_INODE};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// TODO: Implement fstat.
///
/// - syscall ID: 80
/// - 功能：获取文件状态。
/// - 参数：
///   - fd: 文件描述符 -> OSInode -> Inode + offset -> all data
///   - st: 文件状态结构体
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    // st为用户态地址, 需要转换为内核态地址
    let token = current_user_token();
    let user_buffer_start = st as *const u8;
    // 可以正确处理跨页的情况
    let kernel_buffer = translated_byte_buffer(token, user_buffer_start, size_of::<Stat>());

    // 获取文件状态
    let current_task = current_task().unwrap();
    let task_inner = current_task.inner_exclusive_access();
    // 检查文件描述符是否有效
    if fd >= task_inner.fd_table.len() || task_inner.fd_table[fd].is_none() {
        return -1;
    }

    let file = task_inner.fd_table[fd].clone().unwrap();
    let status = file.get_status();
    let status_bytes = unsafe {
        core::slice::from_raw_parts(&status as *const Stat as *const u8, size_of::<Stat>())
    };

    // 将数据写入内核空间的buffer, 从而修改用户空间的st指向的`Stat`结构体
    let mut offset = 0;
    for segment in kernel_buffer {
        let segment_len = segment.len();
        segment.copy_from_slice(&status_bytes[offset..offset + segment_len]);
        offset += segment_len;
    }

    0
}

/// TODO: Implement linkat.
///
/// - syscall ID: 37
/// - 功能：创建一个文件的一个硬链接， linkat标准接口 。
/// - 参数：
///   - old_name：原有文件路径
///   - new_name: 新的链接文件路径。
/// - 说明：
///   - 为了方便，不考虑新文件路径已经存在的情况（属于未定义行为），除非链接同名文件。
/// - 返回值：如果出现了错误则返回 -1，否则返回 0。
/// - 可能的错误: 链接同名文件
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let token = current_user_token();
    let old_name = translated_str(token, old_name);
    let new_name = translated_str(token, new_name);

    // 链接同名文件
    if old_name == new_name {
        return -1;
    }

    // 旧文件不存在或新文件已经存在
    if ROOT_INODE.find(&old_name).is_none() || ROOT_INODE.find(&new_name).is_some() {
        return -1;
    }

    ROOT_INODE.create_link(&new_name, &old_name)
}

/// TODO: Implement unlinkat.
///
/// - syscall ID: 35
/// - 注意考虑使用 unlink 彻底删除文件的情况，此时需要回收inode以及它对应的数据块
/// - 返回值：如果出现了错误则返回 -1，否则返回 0
/// - 可能的错误: 文件不存在
pub fn sys_unlinkat(name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let token = current_user_token();
    let name = translated_str(token, name);

    // 可能的错误: 文件不存在
    if ROOT_INODE.find(&name).is_none() {
        return -1;
    }

    ROOT_INODE.remove_link(&name)
}
