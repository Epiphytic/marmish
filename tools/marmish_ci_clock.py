"""Start the live job budget using even the runner's pre-bootstrap Python."""
import json
import os
import sys
import time
from marmish_platform import continuous_time

if __name__ == '__main__':
    start = continuous_time()
    value = {'start': start, 'monotonic_start': time.monotonic(),
             'wall_start': time.time(), 'deadline': start + 4200}
    descriptor = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        json.dump(value, stream)
