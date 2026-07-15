#!/usr/bin/env python3
"""Retained compatibility entry point for the completed source split."""; from pathlib import Path; ROOT = Path(__file__).resolve().parents[1]; (ROOT / 'crates/tokenzero-mcp/src/tests.rs').read_text()
