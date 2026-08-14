mod block_file;

use another_ext4::{Ext4, InodeMode, EXT4_ROOT_INO};
use block_file::BlockFile;
use std::sync::Arc;

fn main() {
    let image = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/ext4-dir-tail-regression.img".to_string());

    let _ = std::process::Command::new("rm")
        .arg("-f")
        .arg(&image)
        .status();
    let dd = std::process::Command::new("dd")
        .args(["if=/dev/zero", &format!("of={}", image), "bs=1M", "count=512"])
        .status()
        .unwrap();
    assert!(dd.success(), "dd failed");
    let mkfs = std::process::Command::new("mkfs.ext4")
        .args(["-b", "4096"])
        .arg(&image)
        .status()
        .unwrap();
    assert!(mkfs.success(), "mkfs.ext4 failed");

    let ext4 = Ext4::load(Arc::new(BlockFile::open(&image))).unwrap();
    let dir_mode = InodeMode::DIRECTORY | InodeMode::ALL_RWX;
    let file_mode = InodeMode::FILE | InodeMode::ALL_RWX;
    let d = ext4.mkdir(EXT4_ROOT_INO, "d", dir_mode).unwrap();

    for n in 0..360usize {
        let name = format!("f{:03}", n);
        ext4.create(d, &name, file_mode).unwrap();
    }
    ext4.mkdir(d, "vim", dir_mode).unwrap();

    let entries = ext4.listdir(d).unwrap();
    let mut bad = 0usize;
    let mut has_vim = false;
    for entry in &entries {
        let name = entry.name();
        if std::str::from_utf8(name.as_bytes()).is_err() {
            bad += 1;
            eprintln!("bad directory entry name bytes: {:?}", name.as_bytes());
        }
        if name == "vim" {
            has_vim = true;
        }
    }

    // 360 4-byte files + "." + ".." + "vim"
    assert_eq!(entries.len(), 363, "directory entry count mismatch");
    assert_eq!(bad, 0, "invalid directory entries found");
    assert!(has_vim, "vim subdirectory is missing");

    ext4.flush_all();
    println!("OK {}", image);
}
