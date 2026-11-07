// Dedicated action entry: run even when GitHub resolves it through a symlink.
import {main} from './probe.mjs';
process.exitCode = main();
