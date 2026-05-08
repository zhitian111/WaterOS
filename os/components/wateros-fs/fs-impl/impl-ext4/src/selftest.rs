//! ext4 自检：RO 端读固定文本与 ELF 头；RW 端在根目录写入 `/hello` 并由调用方再用只读栈读回校验。
//!
//! 路径常量与根镜像布局耦合（bring-up 镜像中的 `/src/bin/...` 与 `/elf/...`）；更换镜像时需同步调整常量或跳过自检。

use alloc::string::String;
use api_v0::{FsError, FsResult, ReadOnlyFs, SharedFs, SharedRwFs};

// 仅从 ELF 头解析的烟测字段（非完整 loader 语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ElfHeaderInfo {
    class: u8,
    data: u8,
    machine: u16,
    entry: u64,
}

// 仅覆盖 64 位（class==2）与常见 endian；其它组合返回 Unsupported/Corrupt，供日志判断镜像是否可读。
fn parse_elf_header(data: &[u8]) -> FsResult<ElfHeaderInfo> {
    if data.len() < 0x40 {
        return Err(FsError::Io);
    }
    if &data[0..4] != b"\x7fELF" {
        return Err(FsError::Corrupt);
    }
    let class = data[4];
    let data_encoding = data[5];
    let read_u16 = |offset: usize| -> FsResult<u16> {
        let bytes = data.get(offset..offset + 2).ok_or(FsError::Corrupt)?;
        Ok(match data_encoding {
            1 => u16::from_le_bytes([bytes[0], bytes[1]]),
            2 => u16::from_be_bytes([bytes[0], bytes[1]]),
            _ => return Err(FsError::Unsupported),
        })
    };
    let read_u64 = |offset: usize| -> FsResult<u64> {
        let bytes = data.get(offset..offset + 8).ok_or(FsError::Corrupt)?;
        Ok(match data_encoding {
            1 => u64::from_le_bytes(bytes.try_into().map_err(|_| FsError::Corrupt)?),
            2 => u64::from_be_bytes(bytes.try_into().map_err(|_| FsError::Corrupt)?),
            _ => return Err(FsError::Unsupported),
        })
    };
    let machine = read_u16(18)?;
    let entry = match class {
        2 => read_u64(24)?,
        _ => return Err(FsError::Unsupported),
    };
    Ok(ElfHeaderInfo { class, data: data_encoding, machine, entry })
}

/// 用已挂载的只读 ext4 句柄做读取自检。
pub fn ro_self_test(fs: SharedFs) -> FsResult<()> {
    const TEXT_PATH: &str = "/src/bin/000_hello_world.rs";
    const ELF_PATH: &str = "/elf/000_hello_world.elf";

    let fs = fs.lock();

    for path in [TEXT_PATH, ELF_PATH] {
        match fs.metadata(path) {
            Ok(meta) => logging::info!(
                "[fs::ext4][test] metadata OK path={} type={:?} size={} mode={:#o}",
                path,
                meta.node_type,
                meta.size,
                meta.mode
            ),
            Err(err) => logging::warn!(
                "[fs::ext4][test] metadata FAIL path={} err={:?}",
                path,
                err
            ),
        }
    }

    match fs.read_prefix(TEXT_PATH, 96) {
        Ok(text) => {
            let preview = String::from_utf8_lossy(&text);
            let hex_preview_len = text.len().min(32);
            logging::info!(
                "[fs::ext4][test] read OK path={} bytes={} hex_prefix[0..{}]={:02x?} utf8_preview={:?}",
                TEXT_PATH,
                text.len(),
                hex_preview_len,
                &text[..hex_preview_len],
                preview.as_ref()
            );
        }
        Err(err) => logging::warn!(
            "[fs::ext4][test] read FAIL path={} err={:?}",
            TEXT_PATH,
            err
        ),
    }

    match fs.read_prefix(ELF_PATH, 64) {
        Ok(elf_head) => {
            let n = elf_head.len().min(16);
            logging::info!(
                "[fs::ext4][test] read OK path={} bytes={} first16={:02x?}",
                ELF_PATH,
                elf_head.len(),
                &elf_head[..n]
            );
            match parse_elf_header(&elf_head) {
                Ok(info) => logging::info!(
                    "[fs::ext4][test] elf header OK class={} data={} machine={:#x} entry={:#x}",
                    info.class,
                    info.data,
                    info.machine,
                    info.entry
                ),
                Err(err) => logging::warn!(
                    "[fs::ext4][test] elf header PARSE FAIL err={:?} raw64={:02x?}",
                    err,
                    elf_head.as_slice()
                ),
            }
        }
        Err(err) => logging::warn!(
            "[fs::ext4][test] read FAIL path={} err={:?}",
            ELF_PATH,
            err
        ),
    }

    Ok(())
}

/// 用已挂载的读写 ext4 句柄在根目录写入 `name`（内容 `data`）。
pub fn rw_smoke_self_test(rw: SharedRwFs, name: &str, data: &[u8]) -> FsResult<()> {
    let mut rw = rw.lock();
    rw.write_regular_file_at_root(name, data)?;
    logging::info!(
        "[fs::ext4][test] wrote /{} ({} bytes) via ext4 RW",
        name,
        data.len()
    );
    Ok(())
}
