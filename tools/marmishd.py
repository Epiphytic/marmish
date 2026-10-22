#!/usr/bin/env python3
"""Serve allowlisted Marmish command sessions as the local user."""
from marmish_app import guarded_main, server_main
if __name__ == '__main__':
    raise SystemExit(guarded_main(server_main))
