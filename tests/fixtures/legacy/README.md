# Retained v0.1 installer reference

`install-v0.1.sh` is an exact archival fixture for the legacy Python archive,
quickstart and managed-runtime CI tests. It is not the current public installer.
Native installation uses `scripts/install.sh` and the native HTTPS qualification
workflow; Python and uv are not required by the native product.

Provenance:

- Source: `73ed2b41b64ecf614d245668e3ab3b2e5b753895:scripts/install.sh`, immediately
  before the native bootstrap-only cutover.
- Last content commit: `f609b6ccdc9302aeec661a2ae3692d3ef68b86fa`.
- Git blob: `5294acb6cc474a89e4c2de4f51c9f429b188d023`.
- SHA256: `eac417b4748f0252adef82fe18c1260ef744523ec0ed6b624927dc6b803d0768`.
- Bytes unchanged; stored without executable mode. Tests invoke it explicitly
  through `sh` or load its embedded verifier. No loader may fall back to the
  public native bootstrap when this fixture is missing.

The archive suite pins the bytes. Keep this fixture frozen; a change requires
explicit legacy-reference provenance and review. Do not recommend it for new
installations or count its tests as native installer qualification.
