#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
按字典序删除指定文件名区间内的所有文件（包含边界）
用法: python script.py <起始文件名> <结束文件名> [目录路径]
"""

import os
import sys


def delete_file_range(start_name, end_name, directory='.', dry_run=True):
    """
    按字典序删除从 start_name 到 end_name 之间的所有文件（包含两端）
    
    参数:
        start_name: 起始文件名，若不存在则从第一个文件开始
        end_name:   结束文件名，若不存在则到最后一个文件结束
        directory:  目标目录，默认为当前目录
        dry_run:    True 时只预览不删除，False 时真正执行删除
    """
    # 获取目录下所有普通文件（排除子目录），并按字典序排序
    try:
        all_entries = os.listdir(directory)
    except FileNotFoundError:
        print(f"错误: 目录不存在: {directory}")
        return
    except PermissionError:
        print(f"错误: 没有权限访问目录: {directory}")
        return

    files = sorted([f for f in all_entries if os.path.isfile(os.path.join(directory, f))])
    
    if not files:
        print("该目录下没有文件")
        return

    print(f"目录 '{os.path.abspath(directory)}' 下共有 {len(files)} 个文件（已按字典序排列）")
    print("-" * 50)

    # 确定起始索引
    try:
        start_idx = files.index(start_name)
        print(f"起始文件 '{start_name}' 存在，位于第 {start_idx + 1} 位")
    except ValueError:
        start_idx = 0
        print(f"起始文件 '{start_name}' 不存在，将从第 1 个文件 '{files[0]}' 开始")

    # 确定结束索引
    try:
        end_idx = files.index(end_name)
        print(f"结束文件 '{end_name}' 存在，位于第 {end_idx + 1} 位")
    except ValueError:
        end_idx = len(files) - 1
        print(f"结束文件 '{end_name}' 不存在，将到最后一个文件 '{files[-1]}' 结束")

    # 如果起始在结束之后，自动交换（保证区间有效）
    if start_idx > end_idx:
        start_idx, end_idx = end_idx, start_idx
        print("注意: 起始位置在结束位置之后，已自动交换区间边界")

    # 提取目标文件列表
    targets = files[start_idx:end_idx + 1]
    
    if not targets:
        print("没有需要处理的文件")
        return

    print("-" * 50)
    print(f"即将{'【预览】' if dry_run else '【删除】'}以下 {len(targets)} 个文件:")
    for i, f in enumerate(targets, 1):
        filepath = os.path.join(directory, f)
        size = os.path.getsize(filepath)
        print(f"  {i}. {f}  ({size:,} 字节)")

    if dry_run:
        print("-" * 50)
        print("当前为预览模式，未实际删除。如需真正删除，请添加 --exec 参数")
        return

    # 执行删除
    print("-" * 50)
    deleted = 0
    failed = 0
    for f in targets:
        filepath = os.path.join(directory, f)
        try:
            os.remove(filepath)
            print(f"  ✓ 已删除: {f}")
            deleted += 1
        except Exception as e:
            print(f"  ✗ 删除失败: {f} — {e}")
            failed += 1

    print("-" * 50)
    print(f"操作完成: 成功删除 {deleted} 个，失败 {failed} 个")


def main():
    # 解析参数
    args = sys.argv[1:]
    
    if len(args) < 2:
        print("用法: python script.py <起始文件名> <结束文件名> [目录路径] [--exec]")
        print("示例:")
        print("  python script.py aaa.txt zzz.txt          # 预览模式")
        print("  python script.py aaa.txt zzz.txt --exec   # 真正删除")
        print("  python script.py a.txt b.txt ./mydir      # 指定目录")
        sys.exit(1)

    start_name = args[0]
    end_name = args[1]
    directory = '.'
    dry_run = True

    # 解析可选参数
    for arg in args[2:]:
        if arg == '--exec':
            dry_run = False
        elif not arg.startswith('--'):
            directory = arg
        else:
            print(f"未知参数: {arg}")
            sys.exit(1)

    delete_file_range(start_name, end_name, directory, dry_run)


if __name__ == '__main__':
    main()
