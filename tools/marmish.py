#!/usr/bin/env python3
"""Run noninteractive commands over explicitly configured Marmot relays."""
from marmish_app import client_main, guarded_main
if __name__ == '__main__':
    raise SystemExit(guarded_main(client_main))
