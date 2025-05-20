use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task, TaskStatus};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

/// mutex create syscall
///
/// - 返回值为创建的互斥锁的id(PCBInner.mutex_list的index)
/// - mutex_id不可能为负数, 这里用isize作为数据类型是为了syscall的一致性
/// - 互斥锁为特殊的general semaphore, 即Binary Semaphore, 其信号量计数只有0或1
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 创建Mutex资源
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };

    // 在PCBInner.mutex_list中插入Mutex资源
    let mutex_id = if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() - 1
    };

    // 在PCBInner.available_mutex_list中插入可用的Mutex资源
    if process_inner.deadlock_detect {
        process_inner.available_mutex_list.insert(mutex_id, 1);
    }

    mutex_id as isize
}

/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 死锁检测逻辑
    if process_inner.deadlock_detect {
        // 如果可用Mutex资源=0, 则检测失败, 认为会引发死锁, 拒绝本次加锁
        if process_inner.available_mutex_list[&mutex_id] == 0 {
            return -0xDEAD;
        }
        // 如果可用Mutex资源>0, 则检测通过, 允许加锁, 同时资源减少1
        process_inner
            .available_mutex_list
            .entry(mutex_id)
            .and_modify(|number| *number -= 1);
    }

    // 取得Mutex资源的引用
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    // 为Mutex资源加锁
    mutex.lock();

    0
}

/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 如果进程未开启死锁检测, 则忽略available_mutex_list(始终为空)
    if process_inner.deadlock_detect {
        process_inner
            .available_mutex_list
            .entry(mutex_id)
            .and_modify(|number| *number += 1);
    }

    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();

    0
}

/// semaphore create syscall
///
/// - 类似[`sys_mutex_create`]
/// - 创建一般信号量(general semaphore), 对应临界资源数为`resource_count`
/// - 返回值为创建的信号量的的id(PCBInner.semaphore_list的index)
/// - semaphore_id不可能为负数, 这里用isize作为数据类型是为了syscall的一致性
pub fn sys_semaphore_create(resource_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 创建general Semaphore并插入PCBInner.Semaphore_list
    let semaphore_id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(resource_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(resource_count))));
        process_inner.semaphore_list.len() - 1
    };

    if process_inner.deadlock_detect {
        process_inner
            .available_semaphore_list
            .insert(semaphore_id, resource_count as isize);
    }

    semaphore_id as isize
}

/// semaphore up syscall
pub fn sys_semaphore_up(semaphore_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    if process_inner.deadlock_detect {
        process_inner
            .available_semaphore_list
            .entry(semaphore_id)
            .and_modify(|number| *number += 1);
    }

    let sem = Arc::clone(process_inner.semaphore_list[semaphore_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();

    0
}

/// semaphore down syscall
pub fn sys_semaphore_down(semaphore_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    //
    if process_inner.deadlock_detect {
        let ready_tasks_number = process_inner
            .tasks
            .iter()
            .filter_map(|task| task.as_ref())
            .filter(|task| task.inner_exclusive_access().task_status == TaskStatus::Ready)
            .count() as isize;
        if process_inner.available_semaphore_list[&semaphore_id] <= 0 && ready_tasks_number <= 1 {
            return -0xDEAD;
        }

        process_inner
            .available_semaphore_list
            .entry(semaphore_id)
            .and_modify(|number| *number -= 1);
    }

    let semaphore = Arc::clone(process_inner.semaphore_list[semaphore_id].as_ref().unwrap());
    drop(process_inner);
    semaphore.down();

    0
}

/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

/// enable deadlock detection syscall
///
/// TODO: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");

    let current_process = current_process();
    let mut current_process_inner = current_process.inner_exclusive_access();

    if enabled == 1 {
        current_process_inner.deadlock_detect = true;
        0
    } else if enabled == 0 {
        current_process_inner.deadlock_detect = false;
        0
    } else {
        -1
    }
}
