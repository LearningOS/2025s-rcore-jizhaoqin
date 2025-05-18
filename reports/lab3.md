# chapter 5 report

## 实现功能总结

```rust
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize
pub fn sys_munmap(start: usize, len: usize) -> isize 
```
- 同ch4相同, 只是进行用户任务的内存映射和取消映射时, 改为直接访问当前`Task`, 而非通过`TASK_MANAGER`访问

```rust
fn sys_spawn(path: *const u8) -> isize
```
- 实现与`sys_fork()`类似, 不同之处在与直接从elf文件创建并映射内存空间, 并挂载到当前线程下, 而不是复制父进程的内存空间

```rust
fn sys_set_priority(prio: isize) -> isize
```
- 在TCB中加入`priority`以及`stride`, 调整`TaskManager`的进程`fetch`逻辑, 以每次取得`stride`最小的进程, 实现简单的优先级调度策略, 并且进程每次执行都更新其`stride`.

## 问答作业

- stride 算法原理非常简单，但是有一个比较大的问题。例如两个 pass = 10 的进程，使用 8bit 无符号整形储存 stride， p1.stride = 255, p2.stride = 250，在 p2 执行一个时间片后，理论上下一次应该 p1 执行。
- 实际情况是轮到 p1 执行吗？为什么？
  - 下一次还是p2执行, 因为u8加法溢出导致本次执行后`p2.stride = 4 < p1.stride`
- 我们之前要求进程优先级 >= 2 其实就是为了解决这个问题。可以证明， 在不考虑溢出的情况下 , 在进程优先级全部 >= 2 的情况下，如果严格按照算法执行，那么 STRIDE_MAX – STRIDE_MIN <= BigStride / 2
- 为什么？尝试简单说明（不要求严格证明）为什么？尝试简单说明（不要求严格证明）
  - 由于所有进程优先级都>=2, 那么所有进程的stride的移动步长都<=BigStride/2.
  - 若在某一时刻, 满足STRIDE_MAX – STRIDE_MIN <= BigStride / 2, 执行一次进程后, 显然依然满足此条件, 也就意味着之后每一次执行都满足不等式
  - 而创建进程时所有`stride=0`, 满足条件, 所以不等式总是成立
- 已知以上结论，考虑溢出的情况下，可以为 Stride 设计特别的比较器，让 `BinaryHeap<Stride>` 的 pop 方法能返回真正最小的 Stride。补全下列代码中的 partial_cmp 函数，假设两个 Stride 永远不会相等。

```rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.0 < other.0 {
            if (other.0 - self.0) < (1u64 << 63) {
                Some(Ordering::Less) // self < other
            } else {
                Some(Ordering::Greater) // self > other
            }
        } else {
            if (self.0 - other.0) < (1u64 << 63) {
                Some(Ordering::Greater) // self > other
            } else {
                Some(Ordering::Less) // self < other
            }
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false // 题目假设 stride 永不相等
    }
}
```
> TIPS: 使用 8 bits 存储 stride, BigStride = 255, 则: (125 < 255) == false, (129 < 255) == true

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    > 群友

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    > chatgpt

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。