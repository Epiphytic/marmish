"""Owned one-shot Nostr bootstrap pipe; its payload never becomes configuration."""
import os
import subprocess

from marmish_transport_sandbox import SandboxError

MAGIC = b'MRMNST01'
FRAME_SIZE = 72


def wipe(frame):
    if isinstance(frame, bytearray):
        frame[:] = b'\0' * len(frame)


def open_pipe(frame):
    """Consume and wipe an owned frame; return a prefilled reader or DEVNULL."""
    if frame is None:
        return subprocess.DEVNULL
    reader = writer = None
    try:
        if type(frame) is not bytearray or len(frame) != FRAME_SIZE or frame[:8] != MAGIC:
            raise SandboxError('invalid_bootstrap_frame')
        reader, writer = os.pipe()
        os.set_inheritable(reader, False)
        os.set_inheritable(writer, False)
        # 72 bytes fits PIPE_BUF; the writer is closed before any child executes.
        if os.write(writer, frame) != FRAME_SIZE:
            raise SandboxError('bootstrap_pipe_write')
        os.close(writer)
        writer = None
        result, reader = reader, None
        return result
    finally:
        wipe(frame)
        for descriptor in (reader, writer):
            if descriptor is not None:
                os.close(descriptor)
