# chapter 3 report

## 实现功能的总结

```rust
fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize
```

- API要求
  - `trace_request == 0`: 读取地址`id`处的一个`u8`的值
  - `trace_request == 1`: 将`data`转为`u8`, 写入地址`id`处
  - `trace_request == 2`: 查询当前人物的系统调用编号为`id`的调用次数, 
  - 其他情况: 返回`-1`
- 实现:
  - 给结构体`TaskManagerInner`添加`syscall_counter`成员, 记录每个任务的每个系统调用的调用次数
  - 给全局静态变量`TASK_MANAGER`添加`get_syscall_count`和`increase_syscall_count`方法
  - 在每次调用`syscall()`函数时, 首先调用`TASK_MANAGER.increase_syscall_count()`实现计数

## 简答作业

#### 1. 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。 请同学们可以自行测试这些内容（运行 三个 bad 测例 (ch2b_bad_*.rs) ）， 描述程序出错行为，同时注意注明你使用的 sbi 及其版本。
   - 如下第一个试图写入未进行分页映射的非法内存, 报`PageFault`
   - 第二个是执行没有权限的指令
   - 第三个是访问无权限的寄存器
   - sbi版本: `0.4.0`
```
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```
#### 2. 深入理解 trap.S 中两个函数 __alltraps 和 __restore 的作用，并回答如下问题:

1. L40：刚进入 __restore 时，sp 代表了什么值。请指出 __restore 的两种使用情景。
 - 刚进入 `__restore` 时, sp代表内核栈中 `TrapContext` 的起始位置
 - 两种使用情景: 
   - 用户态触发`trap`, 内核处理完后返回用户态
   - 首次执行用户态程序时用来初始化上下文
2. L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。
```asm
ld t0, 32*8(sp)
ld t1, 33*8(sp)
ld t2, 2*8(sp)
csrw sstatus, t0
csrw sepc, t1
csrw sscratch, t2
```
 - `sstatus`: 控制返回的特权级（S → U）、是否启用中断等
 - `sepc`: 设置 sret 跳转回的用户程序地址
 - `sscratch`: 保存用户态的栈指针，确保 trap/返回时能正确切换栈指针
3. L50-L56：为何跳过了 x2 和 x4?
```asm
ld x1, 1*8(sp)
ld x3, 3*8(sp)
.set n, 5
.rept 27
   LOAD_GP %n
   .set n, n+1
.endr
```
- x2(sp): 已通过 `sscratch` 保存并恢复，无需重复保存
- x4(tp): 用户程序不使用 `tp`线程指针（Thread Pointer），省略保存
4. L60：该指令之后，sp 和 sscratch 中的值分别有什么意义？
```asm
csrrw sp, sscratch, sp
```
-  `csrrw` 交换 `sp` 与 `sscratch`的值
-  执行该指令后, `sp`为用户态的栈指针, `sscratch`为内核态的栈指针
5. __restore：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？
- 发生在`sret`指令
- 执行后进入用户态的原因:
  - `sepc` 设置了返回的程序地址
  - 已经设置`sstatus.SPP = 0`(表示trap来自用户态), 告诉硬件返回到用户态
6. L13：该指令之后，sp 和 sscratch 中的值分别有什么意义？
```asm
csrrw sp, sscratch, sp
```
-  `csrrw` 交换 `sp` 与 `sscratch`的值
-  执行该指令后, `sp`为内核态的栈指针, `sscratch`为用户态的栈指针
7. 从 U 态进入 S 态是哪一条指令发生的？
- `ecall`: 环境调用指令，用于请求操作系统或运行时环境提供服务

## 荣誉准则
1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    > 无

2. 此外，我也参考了以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    > chatgpt

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。