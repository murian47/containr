#!/usr/bin/env python3
"""
Codex template-edit addon example for containr.

Input: JSON on stdin (containr addon protocol payload)
Output: JSON status on stdout
"""

from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path


def emit(obj: dict, exit_code: int = 0) -> None:
    print(json.dumps(obj, ensure_ascii=True))
    raise SystemExit(exit_code)


def load_payload() -> dict:
    raw = sys.stdin.read()
    if not raw.strip():
        emit({"ok": False, "error": "empty stdin payload"}, 1)
    try:
        return json.loads(raw)
    except json.JSONDecodeError as exc:
        emit({"ok": False, "error": f"invalid JSON payload: {exc}"}, 1)


def resolve_target(payload: dict) -> tuple[str, str, Path]:
    ctx = payload.get("context") or {}
    templates_dir = (ctx.get("templates_dir") or "").strip()
    if not templates_dir:
        emit({"ok": False, "error": "templates_dir missing in payload"}, 1)

    selected = ctx.get("selected") or []
    target_kind = ""
    target_name = ""
    for item in selected:
        kind = (item.get("kind") or "").strip()
        name = (item.get("name") or "").strip()
        if kind in ("template", "net_template") and name:
            target_kind = kind
            target_name = name
            break

    if not target_kind or not target_name:
        emit({"ok": False, "error": "no selected template or network template"}, 1)

    base = Path(templates_dir)
    if target_kind == "template":
        target_file = base / "stacks" / target_name / "compose.yaml"
    else:
        target_file = base / "networks" / target_name / "network.json"

    if not target_file.is_file():
        emit(
            {
                "ok": False,
                "error": "target file not found",
                "file": str(target_file),
            },
            1,
        )

    return target_kind, target_name, target_file


def build_prompt(payload: dict, target_file: Path) -> str:
    args = payload.get("args") or []
    args_joined = " ".join(str(arg) for arg in args if arg is not None).strip()
    if not args_joined:
        args_joined = "Improve this template while preserving behavior."
    return (
        f"Edit file '{target_file}'. {args_joined} "
        "Return only the updated file content."
    )


def resolve_command(template: str, target_file: Path, prompt: str) -> str:
    return (
        template.replace("{file}", shlex.quote(str(target_file)))
        .replace("{prompt}", shlex.quote(prompt))
        .strip()
    )


def main() -> None:
    payload = load_payload()
    _, _, target_file = resolve_target(payload)
    prompt = build_prompt(payload, target_file)

    cmd_template = os.environ.get("CONTAINR_CODEX_CMD", "").strip()
    if not cmd_template:
        emit(
            {
                "ok": True,
                "mode": "dry-run",
                "file": str(target_file),
                "hint": "Set CONTAINR_CODEX_CMD with {file} and {prompt} placeholders to enable execution.",
                "example": "codex --file {file} --prompt {prompt}",
            },
            0,
        )

    resolved = resolve_command(cmd_template, target_file, prompt)
    if not resolved:
        emit({"ok": False, "error": "resolved command is empty"}, 1)

    proc = subprocess.run(["/bin/sh", "-lc", resolved], check=False)

    if proc.returncode != 0:
        emit(
            {
                "ok": False,
                "mode": "executed",
                "file": str(target_file),
                "exit_code": proc.returncode,
            },
            1,
        )

    emit({"ok": True, "mode": "executed", "file": str(target_file)}, 0)


if __name__ == "__main__":
    main()
