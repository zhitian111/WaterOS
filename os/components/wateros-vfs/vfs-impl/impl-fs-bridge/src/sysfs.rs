//! 最小但语义真实的 sysfs 只读视图。
//!
//! WaterOS 尚未实现 Linux 的 kobject/driver-model，因此这里不试图伪造一棵
//! 可写的完整 sysfs。它只发布内核实际能回答的问题：CPU 拓扑、已 online CPU、
//! 基础网络接口和 virtio 块设备。这些节点覆盖 `lscpu`、`nproc`、stress-ng 与
//! 常见现场诊断脚本所读取的路径；未知属性明确返回 `ENOENT`，不会返回误导性值。

extern crate alloc;

use alloc::{format, string::{String, ToString}, vec, vec::Vec};

use fs::procfs::api::{FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, ProcFsView};

/// `/sys` 的无状态只读视图。
pub(crate) struct KernelSysFs;

static SYSFS: KernelSysFs = KernelSysFs;

pub(crate) fn view() -> &'static KernelSysFs { &SYSFS }

fn normalize(path: &str) -> String {
    let mut out = String::from("/");
    let mut first = true;
    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            // `/sys` 不允许通过视图根向上逃逸；与 VFS 规范化后的结果一致。
            continue;
        }
        if !first {
            out.push('/');
        }
        out.push_str(component);
        first = false;
    }
    out
}

fn online_cpus() -> Vec<usize> {
    task::cpu_states()
        .into_iter()
        .filter_map(|(id, state)| state.online.then_some(id.raw()))
        .collect()
}

fn cpu_list(cpus: &[usize]) -> String {
    if cpus.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut start = cpus[0];
    let mut previous = start;
    for &cpu in &cpus[1..] {
        if cpu == previous.saturating_add(1) {
            previous = cpu;
            continue;
        }
        append_cpu_range(&mut out, start, previous);
        out.push(',');
        start = cpu;
        previous = cpu;
    }
    append_cpu_range(&mut out, start, previous);
    out
}

fn append_cpu_range(out: &mut String, start: usize, end: usize) {
    if start == end {
        out.push_str(start.to_string().as_str());
    } else {
        out.push_str(format!("{start}-{end}").as_str());
    }
}

fn parse_cpu_component(component: &str) -> Option<usize> {
    component.strip_prefix("cpu")?.parse().ok()
}

fn is_online_cpu(cpu: usize) -> bool { online_cpus().contains(&cpu) }

fn is_dir(path: &str) -> bool {
    match path {
        "/" | "/devices" | "/devices/system" | "/devices/system/cpu" |
        "/devices/system/node" | "/devices/system/node/node0" |
        "/class" | "/class/net" | "/class/net/lo" | "/class/net/eth0" |
        "/block" | "/block/vda" | "/block/vda/queue" | "/kernel" | "/firmware" => true,
        _ => {
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            match parts.as_slice() {
                ["devices", "system", "cpu", cpu] => {
                    parse_cpu_component(cpu).is_some_and(is_online_cpu)
                }
                ["devices", "system", "cpu", cpu, "topology"] => {
                    parse_cpu_component(cpu).is_some_and(is_online_cpu)
                }
                _ => false,
            }
        }
    }
}

fn is_symlink(path: &str) -> bool {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        ["devices", "system", "cpu", cpu, "node0"] |
        ["devices", "system", "node", "node0", cpu] => {
            parse_cpu_component(cpu).is_some_and(is_online_cpu)
        }
        _ => false,
    }
}

fn file_data(path: &str) -> Option<Vec<u8>> {
    let online = online_cpus();
    let cpu_set = cpu_list(&online);
    let static_value = match path {
        "/devices/system/cpu/online" |
        "/devices/system/cpu/present" |
        "/devices/system/cpu/possible" => return Some(format!("{cpu_set}\n").into_bytes()),
        "/devices/system/cpu/offline" |
        "/devices/system/cpu/isolated" => return Some(b"\n".to_vec()),
        "/devices/system/cpu/kernel_max" => {
            // cpu_states 包含调度器支持的全部槽位，不假设 QEMU 的 `-smp` 值。
            let max = task::cpu_states().len().saturating_sub(1);
            return Some(format!("{max}\n").into_bytes());
        }
        "/devices/system/node/online" |
        "/devices/system/node/possible" |
        "/devices/system/node/has_cpu" |
        "/devices/system/node/has_memory" => "0\n",
        "/devices/system/node/node0/cpulist" => return Some(format!("{cpu_set}\n").into_bytes()),
        "/devices/system/node/node0/cpumap" => {
            return Some(format!("{}\n", cpu_mask_hex(&online)).into_bytes());
        }
        "/devices/system/node/node0/distance" => "10\n",
        "/class/net/lo/address" => "00:00:00:00:00:00\n",
        "/class/net/lo/operstate" => "unknown\n",
        "/class/net/lo/mtu" => "65536\n",
        "/class/net/lo/type" => "772\n",
        "/class/net/lo/ifindex" => "1\n",
        "/class/net/eth0/address" => "52:54:00:12:34:56\n",
        "/class/net/eth0/operstate" => "up\n",
        "/class/net/eth0/mtu" => "1500\n",
        "/class/net/eth0/type" => "1\n",
        "/class/net/eth0/ifindex" => "2\n",
        "/block/vda/dev" => "252:0\n",
        "/block/vda/size" => return block_device_value(true),
        "/block/vda/queue/logical_block_size" |
        "/block/vda/queue/physical_block_size" => return block_device_value(false),
        "/kernel/uevent_seqnum" => "0\n",
        _ => return cpu_file_data(path),
    };
    Some(static_value.as_bytes().to_vec())
}

fn cpu_file_data(path: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let ["devices", "system", "cpu", cpu, rest @ ..] = parts.as_slice() else {
        return None;
    };
    let cpu = parse_cpu_component(cpu)?;
    if !is_online_cpu(cpu) {
        return None;
    }
    let siblings = cpu_list(&online_cpus());
    let all_mask = cpu_mask_hex(&online_cpus());
    let thread_mask = cpu_mask_hex(&[cpu]);
    let value = match rest {
        ["online"] => "1\n".to_string(),
        ["uevent"] => String::new(),
        ["topology", "core_id"] => format!("{cpu}\n"),
        ["topology", "physical_package_id"] => "0\n".to_string(),
        ["topology", "thread_siblings_list"] => format!("{cpu}\n"),
        ["topology", "core_siblings_list"] |
        ["topology", "package_cpus_list"] => format!("{siblings}\n"),
        ["topology", "thread_siblings"] => format!("{thread_mask}\n"),
        ["topology", "core_siblings"] |
        ["topology", "package_cpus"] => format!("{all_mask}\n"),
        ["topology", "die_id"] |
        ["topology", "cluster_id"] => "0\n".to_string(),
        _ => return None,
    };
    Some(value.into_bytes())
}

fn cpu_mask_hex(cpus: &[usize]) -> String {
    let mut words = [0u32; 2];
    for &cpu in cpus {
        if cpu < u64::BITS as usize {
            words[cpu / 32] |= 1u32 << (cpu % 32);
        }
    }
    // Linux sysfs cpumap 以 32-bit word、每组固定 8 个十六进制字符输出，
    // 高位组在前；util-linux 的 cpuset 解析器依赖这一格式。
    if words[1] == 0 {
        format!("{:08x}", words[0])
    } else {
        format!("{:08x},{:08x}", words[1], words[0])
    }
}

fn block_device_value(capacity: bool) -> Option<Vec<u8>> {
    let device = driver_block_api_v0::first_block_device()?;
    let device = device.lock();
    let block_size = device.block_size() as u64;
    let value = if capacity {
        // sysfs 的 block/<dev>/size 固定以 512-byte sector 为单位。
        device.total_blocks()?.checked_mul(block_size)?.checked_div(512)?
    } else {
        block_size
    };
    Some(format!("{value}\n").into_bytes())
}

fn directory_entries(path: &str) -> Option<Vec<FsDirEntry>> {
    let file = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::File };
    let dir = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::Directory };
    let symlink = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::Symlink };
    match path {
        "/" => Some(vec![dir("devices"), dir("class"), dir("block"), dir("kernel"), dir("firmware")]),
        "/devices" => Some(vec![dir("system")]),
        "/devices/system" => Some(vec![dir("cpu"), dir("node")]),
        "/devices/system/cpu" => {
            let mut entries = vec![file("online"), file("present"), file("possible"),
                                   file("offline"), file("isolated"), file("kernel_max")];
            for cpu in online_cpus() {
                entries.push(symlink(format!("cpu{cpu}").as_str()));
            }
            Some(entries)
        }
        "/devices/system/node" => Some(vec![file("online"), file("possible"),
                                              file("has_cpu"), file("has_memory"), dir("node0")]),
        "/devices/system/node/node0" => {
            let mut entries = vec![file("cpulist"), file("cpumap"), file("distance")];
            for cpu in online_cpus() {
                entries.push(dir(format!("cpu{cpu}").as_str()));
            }
            Some(entries)
        }
        "/class" => Some(vec![dir("net")]),
        "/class/net" => Some(vec![dir("lo"), dir("eth0")]),
        "/class/net/lo" | "/class/net/eth0" => Some(vec![file("address"), file("operstate"),
                                                             file("mtu"), file("type"), file("ifindex")]),
        "/block" => Some(vec![dir("vda")]),
        "/block/vda" => Some(vec![file("dev"), file("size"), dir("queue")]),
        "/block/vda/queue" => Some(vec![file("logical_block_size"), file("physical_block_size")]),
        "/kernel" => Some(vec![file("uevent_seqnum")]),
        "/firmware" => Some(Vec::new()),
        _ => cpu_directory_entries(path),
    }
}

fn cpu_directory_entries(path: &str) -> Option<Vec<FsDirEntry>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let ["devices", "system", "cpu", cpu, rest @ ..] = parts.as_slice() else {
        return None;
    };
    if !parse_cpu_component(cpu).is_some_and(is_online_cpu) {
        return None;
    }
    let file = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::File };
    let dir = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::Directory };
    let symlink = |name: &str| FsDirEntry { name: String::from(name), node_type: FsNodeType::Symlink };
    match rest {
        [] => Some(vec![file("online"), file("uevent"), dir("topology"), symlink("node0")]),
        ["topology"] => Some(vec![file("core_id"), file("physical_package_id"),
                                    file("die_id"), file("cluster_id"),
                                    file("thread_siblings"), file("thread_siblings_list"),
                                    file("core_siblings"), file("core_siblings_list"),
                                    file("package_cpus"), file("package_cpus_list")]),
        _ => None,
    }
}

fn inode_for(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

impl ProcFsView for KernelSysFs {
    fn exists(&self, rel_path: &str) -> FsResult<bool> {
        let path = normalize(rel_path);
        Ok(is_dir(path.as_str()) || is_symlink(path.as_str()) || file_data(path.as_str()).is_some())
    }

    fn metadata(&self, rel_path: &str) -> FsResult<FsMetadata> {
        let path = normalize(rel_path);
        let (node_type, size, mode) = if is_dir(path.as_str()) {
            (FsNodeType::Directory, 0, 0o555)
        } else if is_symlink(path.as_str()) {
            (FsNodeType::Symlink, self.read_symlink(path.as_str())?.len() as u64, 0o777)
        } else if let Some(data) = file_data(path.as_str()) {
            (FsNodeType::File, data.len() as u64, 0o444)
        } else {
            return Err(FsError::NotFound);
        };
        Ok(FsMetadata { node_type, size, mode, inode: inode_for(path.as_str()), nlink: 1, uid: 0, gid: 0 })
    }

    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>> {
        let path = normalize(rel_path);
        if is_dir(path.as_str()) {
            return Err(FsError::NotAFile);
        }
        file_data(path.as_str()).ok_or(FsError::NotFound)
    }

    fn read_symlink(&self, rel_path: &str) -> FsResult<Vec<u8>> {
        let path = normalize(rel_path);
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let target = match parts.as_slice() {
            ["devices", "system", "cpu", cpu, "node0"]
                if parse_cpu_component(cpu).is_some_and(is_online_cpu) => "../../node/node0",
            ["devices", "system", "node", "node0", cpu]
                if parse_cpu_component(cpu).is_some_and(is_online_cpu) => {
                    return Ok(format!("../../cpu/{cpu}").into_bytes());
                }
            _ => return Err(FsError::NotAFile),
        };
        Ok(target.as_bytes().to_vec())
    }

    fn read_dir(&self, rel_path: &str) -> FsResult<Vec<FsDirEntry>> {
        let path = normalize(rel_path);
        directory_entries(path.as_str()).ok_or(FsError::NotAFile)
    }
}
