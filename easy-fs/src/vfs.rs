use core::mem::size_of;

use crate::BLOCK_SIZE;

use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIR_ENTRY_SIZE,
};
use alloc::vec::Vec;
use alloc::{string::String, sync::Arc};
use spin::{Mutex, MutexGuard};

/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }

    /// Find inode under a disk inode by name
    /// 目前只有root inode为目录文件类型, 会调用这个方法
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory, i.e. root disk inode
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIR_ENTRY_SIZE;

        let mut dir_entry = DirEntry::empty();
        // 遍历目录下的所有文件名, 若有name文件则返回对应的inode id
        for i in 0..file_count {
            assert_eq!(
                // 取出一个目录项(包含文件名和inode id)
                disk_inode.read_at(
                    DIR_ENTRY_SIZE * i,
                    dir_entry.as_bytes_mut(),
                    &self.block_device,
                ),
                DIR_ENTRY_SIZE,
            );
            if dir_entry.name() == name {
                return Some(dir_entry.inode_id());
            }
        }
        None
    }

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }

    /// Increase the size of a disk inode
    /// - 对应的`DiskInode`只能逐渐增大不能逐渐减小
    /// - 除非一次性全删除
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }

    /// Create inode under current inode by name
    /// 目前只有root inode可以创建文件, 所以这里的self是`ROOT_INODE`
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }

        // create a new file
        // alloc a inode with an indirect block
        // 在这里进行分配得到返回的索引节点位图的编码, 也就是此文件的inode id
        let new_inode_id = fs.alloc_inode();

        // 在索引节点区创建并初始化磁盘索引节点`DiskInode`对象, 为`DiskInodeType::File`类型
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                // 其他的文件都是`DiskInodeType::File`类型
                new_inode.initialize(DiskInodeType::File);
            });

        // 在root inode的数据块写入新创建的文件的`DirEntry`对象(即文件名和inode id)
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIR_ENTRY_SIZE;
            let new_size = (file_count + 1) * DIR_ENTRY_SIZE;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);

            // 创建文件索引: 包括文件名和inode id(索引节点编号)
            let dirent = DirEntry::new(name, new_inode_id);
            // 把新创建的文件索引数据(32字节), 写入到当前目录(root inode)的数据块中
            root_inode.write_at(
                file_count * DIR_ENTRY_SIZE,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        // 把缓存写入磁盘
        block_cache_sync_all();
        // 创建内核Inode对象并返回
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIR_ENTRY_SIZE;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIR_ENTRY_SIZE,
                        dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIR_ENTRY_SIZE,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }

    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }

    /// 得到Inode的文件信息
    pub fn get_file_status(&self) -> (u64, u32, bool) {
        let fs = self.fs.lock();
        let inode_size = size_of::<DiskInode>();
        let inodes_per_block = BLOCK_SIZE / inode_size;
        let inode_id = (self.block_id - fs.get_inode_area_start_block()) * inodes_per_block
            + (self.block_offset / inode_size);
        // inode_id as u32

        self.read_disk_inode(|disk_inode| (inode_id as u64, disk_inode.nlink, disk_inode.is_file()))
    }

    /// 为文件(Inode)创建硬连接并命名为`new_name`, self为`ROOT_INODE`
    /// 只有`ROOT_INODE`(目录文件)可以调用此方法(也就是创建文件)
    /// - 这里暂时已经保证旧文件一定存在, 新文件不存在
    pub fn create_link(&self, new_name: &str, old_name: &str) -> isize {
        // 检查是否为目录
        if !self.read_disk_inode(|root_disk_inode| root_disk_inode.is_dir()) {
            return -1;
        }

        self.modify_disk_inode(|root_disk_inode| {
            let mut fs = self.fs.lock();

            // 扩大root disk inode的数据块
            let file_count = (root_disk_inode.size as usize) / DIR_ENTRY_SIZE;
            let new_size = (file_count + 1) * DIR_ENTRY_SIZE;
            self.increase_size(new_size as u32, root_disk_inode, &mut fs);

            // 创建文件索引: 包括文件名和inode id(索引节点编号)
            let inode_id = self.find_inode_id(old_name, root_disk_inode).unwrap();
            let dir_entry = DirEntry::new(new_name, inode_id);

            // 把新创建的文件索引数据(32字节), 写入到当前目录(root inode)的数据块中
            root_disk_inode.write_at(
                file_count * DIR_ENTRY_SIZE,
                dir_entry.as_bytes(),
                &self.block_device,
            );
        });

        // 增加硬连接数
        let inode = self.find(old_name).unwrap();
        inode.modify_disk_inode(|file_inode| file_inode.nlink += 1);

        0
    }

    /// 删除一个硬连接(文件)
    /// 目前只有`ROOT_INODE`(目录文件)可以调用此方法
    /// - 调用时已确保name存在
    pub fn remove_link(&self, name: &str) -> isize {
        // 检查是否为目录
        if !self.read_disk_inode(|root_disk_inode| root_disk_inode.is_dir()) {
            return -1;
        }

        // 先从root inode数据块中取出文件Inode, 不然后面删除就不能从root inode找了
        let inode = self.find(name).unwrap();

        // 先删除root inode数据块里储存的文件信息
        self.modify_disk_inode(|root_disk_inode| {
            let file_count = (root_disk_inode.size as usize) / DIR_ENTRY_SIZE;

            let mut dir_entry = DirEntry::empty();
            // 遍历目录下的所有文件
            for i in 0..file_count {
                assert_eq!(
                    // 取出一个目录项(包含文件名和inode id)
                    root_disk_inode.read_at(
                        DIR_ENTRY_SIZE * i,
                        dir_entry.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIR_ENTRY_SIZE,
                );
                // 如果找到就向其中写入空`DirEntry`
                if dir_entry.name() == name {
                    root_disk_inode.write_at(
                        DIR_ENTRY_SIZE * i,
                        DirEntry::empty().as_bytes(),
                        &self.block_device,
                    );
                    break;
                }
            }
        });

        // 再修改file disk inode里的信息(nlink==0时删除)
        // 减少硬连接数
        inode.modify_disk_inode(|file_inode| file_inode.nlink -= 1);
        if inode.read_disk_inode(|file_disk_inode| file_disk_inode.nlink == 0) {
            inode.clear();
        }
        0
    }
}
