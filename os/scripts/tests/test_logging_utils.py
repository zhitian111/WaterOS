#!/usr/bin/env python3
"""验证 Python 脚本共用日志接口的稳定输出格式。"""

from __future__ import annotations

import contextlib
import io
import os
import sys
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from source.logging_utils import info, warning  # noqa: E402


class LoggingUtilsTests(unittest.TestCase):
    def test_explicit_component_and_level(self) -> None:
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            info("配置加载完成 path=config.conf", component="CONFIG")
        self.assertEqual(
            output.getvalue(),
            "[CONFIG][INFO] 配置加载完成 path=config.conf\n",
        )

    def test_environment_component(self) -> None:
        output = io.StringIO()
        previous = os.environ.get("WOS_LOG_COMPONENT")
        os.environ["WOS_LOG_COMPONENT"] = "TEST"
        try:
            with contextlib.redirect_stderr(output):
                warning("测试未完成 exit_code=124")
        finally:
            if previous is None:
                os.environ.pop("WOS_LOG_COMPONENT", None)
            else:
                os.environ["WOS_LOG_COMPONENT"] = previous
        self.assertEqual(output.getvalue(), "[TEST][WARN] 测试未完成 exit_code=124\n")


if __name__ == "__main__":
    unittest.main()
