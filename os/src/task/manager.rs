//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

pub const BIG_STRIDE: usize = 512;

/// A array of `TaskControlBlock` that is thread-safe
///
/// 维护进程队列, 将执行逻辑分给`Processor`
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Create an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }

    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        // 每次添加入队列都更新stride
        task.inner_exclusive_access().update_stride();
        self.ready_queue.push_back(task);
    }

    /// Take a process with min `stride` out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }
        // Find the index with the minimum stride
        let min_stride_index = self
            .ready_queue
            .iter()
            .enumerate()
            .min_by_key(|&(_, task)| task.inner_exclusive_access().stride)
            .map(|(idx, _)| idx)?;
        self.ready_queue.remove(min_stride_index)
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
