// Upload runs in its own owned, bounded tooling child because the official
// artifact SDK does not expose cancellation of an in-flight upload promise.
import {DefaultArtifactClient} from '@actions/artifact';
const [name, file, directory] = process.argv.slice(2);
new DefaultArtifactClient().uploadArtifact(name, [file], directory, {retentionDays: 14})
  .then(async result => {
    if (!Number.isSafeInteger(result.id) || result.id <= 0) throw new Error('invalid_id');
    await new Promise((resolve, reject) => process.send({id: result.id}, error => error ? reject(error) : resolve()));
    process.disconnect();
  }).catch(() => { process.exitCode = 1; if (process.connected) process.disconnect(); });
