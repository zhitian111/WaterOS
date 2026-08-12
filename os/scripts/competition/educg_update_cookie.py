#!/usr/bin/env python3
"""向服务器上的 educg_submit_daemon 推送 educg_session cookie。

用法：
  # 首次：复制并编辑配置
  cp os/scripts/competition/educg_cookie.conf.example os/scripts/competition/educg_cookie.conf

  # 推送 cookie（从浏览器 DevTools 复制 educg_session 值）
  python3 os/scripts/competition/educg_update_cookie.py --session 'A028F4BC...'

  # 交互输入（不回显）
  python3 os/scripts/competition/educg_update_cookie.py

  # 查看服务器状态
  python3 os/scripts/competition/educg_update_cookie.py --status
"""

from __future__ import annotations

import argparse
import getpass
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_CONF = SCRIPT_DIR / "educg_cookie.conf"


def load_conf(path: Path) -> dict[str, Any]:
    if not path.is_file():
        print(
            f"缺少配置文件: {path}\n"
            f"请先执行: cp {SCRIPT_DIR / 'educg_cookie.conf.example'} {path}",
            file=sys.stderr,
        )
        sys.exit(1)
    with path.open(encoding="utf-8") as f:
        return json.load(f)


def http_json(
    method: str,
    url: str,
    payload: dict[str, Any] | None = None,
    *,
    timeout: float = 15.0,
) -> tuple[int, dict[str, Any]]:
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json; charset=utf-8"

    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", errors="replace")
            return resp.status, json.loads(body) if body else {}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(body) if body else {}
        except json.JSONDecodeError:
            parsed = {"error": body or str(exc)}
        return exc.code, parsed
    except urllib.error.URLError as exc:
        print(f"网络错误: {exc.reason}", file=sys.stderr)
        sys.exit(1)


def push_cookie(server_url: str, token: str, session: str) -> int:
    url = server_url.rstrip("/") + "/api/cookie"
    status, resp = http_json(
        "POST",
        url,
        {"educg_session": session.strip(), "token": token},
    )
    if status == 200 and resp.get("ok"):
        preview = resp.get("cookie_preview", "")
        print(f"cookie 更新成功（{preview}）")
        return 0

    print(f"cookie 更新失败（HTTP {status}）: {resp.get('error', resp)}", file=sys.stderr)
    return 1


def show_status(server_url: str) -> int:
    url = server_url.rstrip("/") + "/api/status"
    status, resp = http_json("GET", url)
    if status != 200:
        print(f"查询失败（HTTP {status}）: {resp}", file=sys.stderr)
        return 1

    print(json.dumps(resp, ensure_ascii=False, indent=2))
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="向服务器推送 educg_session cookie")
    parser.add_argument(
        "--conf",
        type=Path,
        default=Path(__import__("os").environ.get("EDUCG_COOKIE_CONF", DEFAULT_CONF)),
        help=f"配置文件路径（默认 {DEFAULT_CONF}）",
    )
    parser.add_argument("--session", help="educg_session 值；省略则交互输入")
    parser.add_argument("--status", action="store_true", help="查看服务器状态")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    conf = load_conf(args.conf)

    server_url = conf.get("server_url", "").strip()
    token = conf.get("auth_token", "").strip()
    if not server_url:
        print("配置缺少 server_url", file=sys.stderr)
        sys.exit(1)
    if not token or token.startswith("与服务器"):
        print("请先在配置文件中设置有效的 auth_token", file=sys.stderr)
        sys.exit(1)

    if args.status:
        raise SystemExit(show_status(server_url))

    session = args.session
    if not session:
        session = getpass.getpass("请输入 educg_session: ").strip()
    if not session:
        print("educg_session 不能为空", file=sys.stderr)
        sys.exit(1)

    raise SystemExit(push_cookie(server_url, token, session))


if __name__ == "__main__":
    main()
