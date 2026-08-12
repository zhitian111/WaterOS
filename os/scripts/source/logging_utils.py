#!/usr/bin/env python3
"""WaterOS Python 脚本共用的结构化日志接口。"""

from __future__ import annotations

import os
import sys
from typing import TextIO


def _component(component: str | None) -> str:
    return component or os.environ.get("WOS_LOG_COMPONENT", "SCRIPT")


def log(
    level: str,
    message: str,
    *,
    component: str | None = None,
    file: TextIO | None = None,
) -> None:
    """按 ``[组件][级别] 信息`` 格式写入一条日志。"""

    print(
        f"[{_component(component)}][{level}] {message}",
        file=sys.stderr if file is None else file,
        flush=True,
    )


def trace(message: str, *, component: str | None = None) -> None:
    log("TRACE", message, component=component)


def debug(message: str, *, component: str | None = None) -> None:
    log("DEBUG", message, component=component)


def info(message: str, *, component: str | None = None) -> None:
    log("INFO", message, component=component)


def warning(message: str, *, component: str | None = None) -> None:
    log("WARN", message, component=component)


def error(message: str, *, component: str | None = None) -> None:
    log("ERROR", message, component=component)
