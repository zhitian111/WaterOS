#!/usr/bin/env python3
"""为 WaterOS 脚本提供汉语 argparse 帮助界面。"""

from __future__ import annotations

import argparse


class ChineseArgumentParser(argparse.ArgumentParser):
    """将 argparse 自动生成的固定说明翻译为汉语。"""

    _REPLACEMENTS = {
        "usage:": "用法：",
        "options:": "选项：",
        "optional arguments:": "可选参数：",
        "positional arguments:": "位置参数：",
        "show this help message and exit": "显示帮助信息并退出",
    }

    @classmethod
    def _translate(cls, text: str) -> str:
        for source, target in cls._REPLACEMENTS.items():
            text = text.replace(source, target)
        return text

    def format_help(self) -> str:
        return self._translate(super().format_help())

    def format_usage(self) -> str:
        return self._translate(super().format_usage())

    def error(self, message: str) -> None:
        self.print_usage(__import__("sys").stderr)
        self.exit(2, f"{self.prog}：参数错误：{message}\n")
