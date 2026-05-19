//! 后端组合 trait：impl 对单一类型实现此面即可被 `active_impl` 选中。

use crate::dev::VfsDevInventory;
use crate::handle::VfsOpenOps;
use crate::mount::VfsMountOps;
use crate::namespace::VfsMountTable;
use crate::root_read::SingleRootReadView;

/// VFS 模块当前阶段的后端能力组合（随路线图扩展时在此追加 supertrait）。
pub trait VfsBackend:
    SingleRootReadView
    + VfsMountOps
    + VfsDevInventory
    + VfsOpenOps
    + VfsMountTable
{
}
